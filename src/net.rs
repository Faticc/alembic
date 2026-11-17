//! Сетевая игра. Два транспорта под одним протоколом:
//! - Steam (тестовый AppID 480 "Spacewar"): лобби + ISteamNetworkingMessages, только на ПК;
//! - прямое соединение по TCP: комнаты в локальной сети находятся сами (UDP), в остальных
//!   случаях входят по IP. Так играют телефон и ПК: клиента Steam на телефоне нет.
//!
//! Схема: хост - единственный источник правды. Клиенты шлют команды строительства,
//! хост применяет их и рассылает всем результат и состояние экономики. Предметы на
//! лентах каждый клиент досчитывает сам - они только для вида.

use crate::world::{Dir, Kind, N_ALL_IDS};
use std::cell::RefCell;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Игрок в сети: SteamID или случайный номер тех, кто пришёл по TCP.
pub type PeerId = u64;

/// Порт комнаты: TCP - игра, UDP - поиск комнат в локальной сети.
pub const PORT: u16 = 24642;
/// Первое сообщение соединения по TCP: метка с версией протокола, за ней номер игрока.
const MAGIC: &[u8; 8] = b"ALEMBIC7";
const ASK: &[u8] = b"ALEMBIC?7";
const ANSWER: &[u8] = b"ALEMBIC!7";
/// Больше этого сообщение быть не может: соединение с мусором закрывается.
const MAX_FRAME: usize = 4 << 20;
const TIMEOUT: f32 = 12.0;
/// Как часто мерить пинг, секунды.
const PING_EVERY: f32 = 1.0;

/// Сглаживание пинга: новое значение входит на треть, чтобы цифра не прыгала.
fn smooth_ping(old: Option<f32>, new: f32) -> f32 {
    old.map_or(new, |o| o + (new - o) * 0.35)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Offline,
    Host,
    Client,
}

#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    Place(Kind, i32, i32, Dir),
    Remove(i32, i32),
    BeltDir(i32, i32, Dir),
    Upgrade(u8),
    LevelUp(i32, i32),
    SetPorts(i32, i32, u8),
    FilterReset(i32, i32),
    /// Настройка постройки: образец фильтра, пропорция, код телепорта, сторона отказа.
    SetCfg(i32, i32, u64),
    /// Слой сигналов: снять деталь, задать её настройку.
    LRemove(i32, i32),
    LCfg(i32, i32, u64),
    /// Гильдии и виток: взять заказ, сменить скин лент, начать новый виток.
    Order(u8),
    Skin(u8),
    Prestige,
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Set(i32, i32, Kind, Dir),
    Clear(i32, i32),
    Dir(i32, i32, Dir),
    Level(i32, i32, u8),
    Ports(i32, i32, u8),
    FilterReset(i32, i32),
    Cfg(i32, i32, u64),
    LClear(i32, i32),
    LCfg(i32, i32, u64),
}

pub struct StateMsg {
    pub money: f64,
    pub level: u32,
    pub delivered: u32,
    pub upgrades: [u8; 8],
    pub income: f32,
    /// Время мира хоста: по нему клиент сверяет фазу переключателей.
    pub time: f64,
    pub discovered: Vec<bool>,
    /// Остальная экономика (рынок, гильдии, мегапроект, виток) - строками, как в сохранении.
    pub extra: String,
}

pub enum NetEvent {
    /// Хосту: игрок подключился, ему нужно отправить мир.
    Hello(PeerId, String),
    /// Хосту: команда от игрока.
    Cmd(Cmd),
    /// Клиенту: снимок мира, изменение постройки, состояние экономики.
    World(String),
    Op(Op),
    /// Клиенту: состояние слоя сигналов хоста (номер такта и состояние деталей по порядку).
    Logic(u64, Vec<(bool, u32, bool)>),
    State(StateMsg),
    Info(String),
    /// Клиенту: связь с хостом потеряна, пора вернуться в свою игру.
    Lost(String),
}

/// Что игрок делает прямо сейчас - это видят остальные.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Presence {
    /// Тайл под курсором.
    pub x: i32,
    pub y: i32,
    /// Выбранная постройка (Kind.0) или 255, если руки пусты.
    pub tool: u8,
    pub dir: u8,
    /// 0 - ничего, 1 - рамка выделения, 2 - вставка из буфера.
    pub mode: u8,
    /// Рамка в тайлах: (x0, y0, x1, y1), включительно.
    pub rect: (i32, i32, i32, i32),
}

fn wr_pres(w: Wr, p: &Presence) -> Wr {
    w.i16(p.x).i16(p.y).u8(p.tool).u8(p.dir).u8(p.mode).i16(p.rect.0).i16(p.rect.1).i16(p.rect.2).i16(p.rect.3)
}

fn rd_pres(r: &mut Rd) -> Option<Presence> {
    Some(Presence {
        x: r.i16()?,
        y: r.i16()?,
        tool: r.u8()?,
        dir: r.u8()? & 3,
        mode: r.u8()?,
        rect: (r.i16()?, r.i16()?, r.i16()?, r.i16()?),
    })
}

pub struct Peer {
    pub id: PeerId,
    pub name: String,
    pub p: Presence,
    silence: f32,
    /// Пинг до этого игрока, мс (знает только хост).
    pub ping_ms: Option<f32>,
}

/// Куда стучаться, чтобы войти в комнату.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RoomAddr {
    /// Лобби Steam.
    Steam(u64),
    /// Комната в локальной сети или по IP.
    Lan(SocketAddr),
}

pub struct LobbyInfo {
    pub addr: RoomAddr,
    pub name: String,
    pub members: usize,
}

/// То, что Steam сообщает сетевому слою, уже без типов Steam.
enum SteamEv {
    Created(Result<(), String>),
    /// Вошли в лобби: номер его хозяина.
    Joined(Result<u64, ()>),
    List(Vec<(u64, String, usize)>),
    JoinRequested(u64),
}

#[cfg(windows)]
mod steam {
    use super::SteamEv;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use steamworks::networking_messages::SessionRequest;
    use steamworks::networking_types::{NetworkingIdentity, SendFlags};
    use steamworks::{
        CallbackHandle, Client, DistanceFilter, GameLobbyJoinRequested, LobbyId, LobbyKey, LobbyType, SteamId,
        StringFilter, StringFilterKind,
    };

    const APP_ID: u32 = 480;
    /// Метка комнаты включает версию протокола: старые и новые сборки друг друга не видят.
    const LOBBY_TAG: &str = "alembic_tycoon_v7";

    enum Raw {
        Created(Result<LobbyId, String>),
        Joined(Result<LobbyId, ()>),
        List(Vec<LobbyId>),
        JoinRequested(LobbyId),
        Session(SessionRequest),
    }

    pub struct Steam {
        client: Client,
        rx: Receiver<Raw>,
        tx: Sender<Raw>,
        _handles: Vec<CallbackHandle>,
        lobby: Option<LobbyId>,
        pub name: String,
        pub id: u64,
    }

    impl Steam {
        /// Подключается к запущенному клиенту Steam.
        pub fn connect() -> Result<Steam, String> {
            let client = Client::init_app(APP_ID).map_err(|e| format!("{:?}", e))?;
            let (tx, rx) = channel();
            let t = tx.clone();
            client.networking_messages().session_request_callback(move |req| {
                let _ = t.send(Raw::Session(req));
            });
            let t = tx.clone();
            let handle = client.register_callback(move |r: GameLobbyJoinRequested| {
                let _ = t.send(Raw::JoinRequested(r.lobby_steam_id));
            });
            let name = client.friends().name();
            let id = client.user().steam_id().raw();
            Ok(Steam { client, rx, tx, _handles: vec![handle], lobby: None, name, id })
        }

        pub fn create_lobby(&self) {
            let tx = self.tx.clone();
            self.client.matchmaking().create_lobby(LobbyType::Public, 8, move |r| {
                let _ = tx.send(Raw::Created(r.map_err(|e| format!("{:?}", e))));
            });
        }

        pub fn request_list(&self) {
            let tx = self.tx.clone();
            let mm = self.client.matchmaking();
            mm.add_request_lobby_list_string_filter(StringFilter(LobbyKey::new("game"), LOBBY_TAG, StringFilterKind::Equal));
            mm.set_request_lobby_list_distance_filter(DistanceFilter::Worldwide);
            mm.request_lobby_list(move |r| {
                let _ = tx.send(Raw::List(r.unwrap_or_default()));
            });
        }

        pub fn join(&self, lobby: u64) {
            let tx = self.tx.clone();
            self.client.matchmaking().join_lobby(LobbyId::from_raw(lobby), move |r| {
                let _ = tx.send(Raw::Joined(r));
            });
        }

        pub fn leave_lobby(&mut self) {
            if let Some(l) = self.lobby.take() {
                self.client.matchmaking().leave_lobby(l);
            }
        }

        pub fn send(&self, to: u64, data: &[u8], reliable: bool) {
            let flags = if reliable { SendFlags::RELIABLE } else { SendFlags::UNRELIABLE }
                | SendFlags::AUTO_RESTART_BROKEN_SESSION;
            let to = NetworkingIdentity::new_steam_id(SteamId::from_raw(to));
            let _ = self.client.networking_messages().send_message_to_user(to, flags, data, 0);
        }

        /// В тестовом приложении Spacewar наших достижений нет - тогда Steam просто отказывает.
        pub fn achievement(&self, id: &str) -> bool {
            let stats = self.client.user_stats();
            stats.achievement(id).set().is_ok() && stats.store_stats().is_ok()
        }

        /// Колбэки Steam и входящие сообщения.
        pub fn poll(&mut self, evs: &mut Vec<SteamEv>, msgs: &mut Vec<(u64, Vec<u8>)>) {
            self.client.run_callbacks();
            let mm = self.client.matchmaking();
            while let Ok(ev) = self.rx.try_recv() {
                match ev {
                    Raw::Created(Ok(l)) => {
                        mm.set_lobby_data(l, "game", LOBBY_TAG);
                        mm.set_lobby_data(l, "name", &self.name);
                        self.lobby = Some(l);
                        evs.push(SteamEv::Created(Ok(())));
                    }
                    Raw::Created(Err(e)) => evs.push(SteamEv::Created(Err(e))),
                    Raw::Joined(Ok(l)) => {
                        self.lobby = Some(l);
                        evs.push(SteamEv::Joined(Ok(mm.lobby_owner(l).raw())));
                    }
                    Raw::Joined(Err(())) => evs.push(SteamEv::Joined(Err(()))),
                    Raw::List(ids) => evs.push(SteamEv::List(
                        ids.into_iter()
                            .take(8)
                            .map(|id| {
                                let name = mm.lobby_data(id, "name").unwrap_or_else(|| "?".to_string());
                                (id.raw(), name, mm.lobby_member_count(id))
                            })
                            .collect(),
                    )),
                    Raw::JoinRequested(l) => evs.push(SteamEv::JoinRequested(l.raw())),
                    Raw::Session(req) => {
                        // Принимаем соединения только от участников нашей комнаты.
                        let known = match (self.lobby, req.remote().steam_id()) {
                            (Some(l), Some(id)) => mm.lobby_members(l).contains(&id),
                            _ => false,
                        };
                        if known {
                            req.accept();
                        }
                    }
                }
            }
            let nm = self.client.networking_messages();
            for _ in 0..8 {
                let batch = nm.receive_messages_on_channel(0, 64);
                if batch.is_empty() {
                    break;
                }
                for m in batch {
                    if let Some(from) = m.identity_peer().steam_id() {
                        msgs.push((from.raw(), m.data().to_vec()));
                    }
                }
            }
        }
    }
}

/// Там, где клиента Steam нет (телефон), остаётся только прямое соединение.
#[cfg(not(windows))]
mod steam {
    use super::SteamEv;

    pub struct Steam {
        pub name: String,
        pub id: u64,
    }

    impl Steam {
        pub fn connect() -> Result<Steam, String> {
            Err("НЕТ НА ЭТОМ УСТРОЙСТВЕ".to_string())
        }
        pub fn create_lobby(&self) {}
        pub fn request_list(&self) {}
        pub fn join(&self, _lobby: u64) {}
        pub fn leave_lobby(&mut self) {}
        pub fn send(&self, _to: u64, _data: &[u8], _reliable: bool) {}
        pub fn achievement(&self, _id: &str) -> bool {
            false
        }
        pub fn poll(&mut self, _evs: &mut Vec<SteamEv>, _msgs: &mut Vec<(u64, Vec<u8>)>) {}
    }
}

use steam::Steam;

// ---------- прямое соединение ----------

/// Соединение по TCP. Сообщения идут подряд, перед каждым - длина (4 байта).
struct Conn {
    /// Номер игрока на том конце; 0 - пока не пришло приветствие.
    id: PeerId,
    stream: TcpStream,
    inbuf: Vec<u8>,
    outbuf: Vec<u8>,
    dead: bool,
    age: f32,
}

impl Conn {
    fn new(stream: TcpStream, my_id: PeerId) -> Option<Conn> {
        let _ = stream.set_nodelay(true);
        stream.set_nonblocking(true).ok()?;
        let mut c = Conn { id: 0, stream, inbuf: Vec::new(), outbuf: Vec::new(), dead: false, age: 0.0 };
        let mut hello = MAGIC.to_vec();
        hello.extend_from_slice(&my_id.to_le_bytes());
        c.queue(&hello);
        Some(c)
    }

    fn queue(&mut self, data: &[u8]) {
        self.outbuf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.outbuf.extend_from_slice(data);
        self.flush();
    }

    fn flush(&mut self) {
        while !self.outbuf.is_empty() && !self.dead {
            match self.stream.write(&self.outbuf) {
                Ok(0) => self.dead = true,
                Ok(n) => {
                    self.outbuf.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.dead = true,
            }
        }
        // Тот конец не читает: копить без конца незачем.
        if self.outbuf.len() > 4 * MAX_FRAME {
            self.dead = true;
        }
    }

    /// Читает всё, что пришло, и отдаёт целые сообщения.
    fn read(&mut self, frames: &mut Vec<Vec<u8>>) {
        let mut buf = [0u8; 16384];
        while !self.dead {
            match self.stream.read(&mut buf) {
                Ok(0) => self.dead = true,
                Ok(n) => self.inbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.dead = true,
            }
        }
        let mut at = 0;
        while self.inbuf.len() - at >= 4 {
            let n = u32::from_le_bytes(self.inbuf[at..at + 4].try_into().unwrap()) as usize;
            if n > MAX_FRAME {
                self.dead = true;
                break;
            }
            if self.inbuf.len() - at - 4 < n {
                break;
            }
            frames.push(self.inbuf[at + 4..at + 4 + n].to_vec());
            at += 4 + n;
        }
        self.inbuf.drain(..at);
    }
}

/// Адрес этого устройства в локальной сети. Соединение UDP ничего не отправляет: система
/// лишь выбирает, с какого адреса она пошла бы наружу.
fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// Имя игрока, когда его не подсказал Steam.
fn device_name() -> String {
    #[cfg(target_os = "android")]
    {
        extern "C" {
            fn __system_property_get(name: *const u8, value: *mut u8) -> i32;
        }
        // PROP_VALUE_MAX = 92.
        let mut buf = [0u8; 96];
        let n = unsafe { __system_property_get(b"ro.product.model\0".as_ptr(), buf.as_mut_ptr()) };
        if n > 0 {
            return String::from_utf8_lossy(&buf[..n as usize]).chars().take(16).collect();
        }
    }
    std::env::var("USERNAME").ok().filter(|s| !s.is_empty()).map_or("ИГРОК".to_string(), |s| s.chars().take(16).collect())
}

/// "192.168.1.5" или "192.168.1.5:24642".
pub fn parse_addr(text: &str) -> Option<SocketAddr> {
    let t = text.trim();
    t.parse::<SocketAddr>().ok().or_else(|| t.parse::<Ipv4Addr>().ok().map(|ip| SocketAddr::new(IpAddr::V4(ip), PORT)))
}

// ---------- сеть ----------

pub struct Net {
    steam: Option<Steam>,
    /// Итог подключения по TCP приходит из фонового потока.
    rx: Receiver<Result<TcpStream, String>>,
    tx: Sender<Result<TcpStream, String>>,
    conns: RefCell<Vec<Conn>>,
    listener: Option<TcpListener>,
    /// Хост: отвечает на поиск комнат. Клиент: рассылает поиск и ждёт ответы.
    udp: Option<UdpSocket>,
    scan_t: f32,
    scan_ping_t: f32,
    /// Адрес хоста в локальной сети - его называют тем, кто входит по IP.
    pub lan_ip: Option<String>,
    pub mode: Mode,
    pub status: String,
    pub my_name: String,
    my_id: PeerId,
    host: Option<PeerId>,
    /// Другие игроки (у клиента - по рассылке хоста, включая самого хоста).
    pub peers: Vec<Peer>,
    pub lobbies: Vec<LobbyInfo>,
    pub busy: bool,
    synced: bool,
    hello_t: f32,
    host_silence: f32,
    cursor: Presence,
    cursor_t: f32,
    state_t: f32,
    clock: std::time::Instant,
    ping_t: f32,
    /// Клиенту: пинг до хоста, мс.
    pub ping_ms: Option<f32>,
}

impl Net {
    pub fn new() -> Net {
        let (tx, rx) = channel();
        Net {
            steam: None,
            rx,
            tx,
            conns: RefCell::new(Vec::new()),
            listener: None,
            udp: None,
            scan_t: 0.0,
            scan_ping_t: 0.0,
            lan_ip: None,
            mode: Mode::Offline,
            status: "НЕ В СЕТИ".to_string(),
            my_name: String::new(),
            my_id: 0,
            host: None,
            peers: Vec::new(),
            lobbies: Vec::new(),
            busy: false,
            synced: false,
            hello_t: 0.0,
            host_silence: 0.0,
            cursor: Presence::default(),
            cursor_t: 0.0,
            state_t: 0.0,
            clock: std::time::Instant::now(),
            ping_t: 0.0,
            ping_ms: None,
        }
    }

    /// Готовит сеть: подключается к клиенту Steam, если он есть, иначе берёт имя устройства
    /// и случайный номер игрока. false - Steam недоступен, игра возможна только напрямую.
    pub fn ensure(&mut self) -> bool {
        if self.steam.is_none() && cfg!(windows) && !cfg!(test) && !self.online() && !self.busy {
            match Steam::connect() {
                Ok(s) => {
                    self.my_name = s.name.clone();
                    self.my_id = s.id;
                    self.status = format!("STEAM: {}", self.my_name);
                    self.steam = Some(s);
                }
                Err(e) => self.status = format!("STEAM НЕДОСТУПЕН ({}): ИГРА ПО СЕТИ ИЛИ IP", e).to_uppercase(),
            }
        }
        if self.my_id == 0 {
            // Старший байт не совпадает ни с одним SteamID.
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
            self.my_id = 0xA1u64 << 56 | (t.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 8).max(1);
            self.my_name = device_name();
            if !cfg!(windows) {
                self.status = "ИГРА ПО WI-FI ИЛИ IP".to_string();
            }
        }
        self.steam.is_some()
    }

    pub fn online(&self) -> bool {
        self.mode != Mode::Offline
    }

    /// Открывает комнату: в локальной сети сразу, в Steam - когда он создаст лобби.
    pub fn host_game(&mut self) {
        self.ensure();
        if self.online() || self.busy {
            return;
        }
        self.listener = TcpListener::bind(("0.0.0.0", PORT)).ok().filter(|l| l.set_nonblocking(true).is_ok());
        self.udp = UdpSocket::bind(("0.0.0.0", PORT)).ok().filter(|s| s.set_nonblocking(true).is_ok());
        self.lan_ip = self.listener.as_ref().and(local_ip()).map(|ip| ip.to_string());
        if let Some(s) = &self.steam {
            self.busy = true;
            self.status = "СОЗДАЮ КОМНАТУ...".to_string();
            s.create_lobby();
        } else if self.listener.is_some() {
            self.become_host();
        } else {
            self.status = format!("НЕ УДАЛОСЬ ОТКРЫТЬ КОМНАТУ: ПОРТ {} ЗАНЯТ", PORT);
        }
    }

    fn become_host(&mut self) {
        self.mode = Mode::Host;
        self.peers.clear();
        self.status = match &self.lan_ip {
            Some(ip) => format!("ТЫ ХОСТ. КОМНАТА ОТКРЫТА, IP {}", ip),
            None => "ТЫ ХОСТ. КОМНАТА ОТКРЫТА".to_string(),
        };
    }

    /// Ищет комнаты: в Steam и рассылкой по локальной сети.
    pub fn refresh(&mut self) {
        self.ensure();
        if self.busy || self.online() {
            return;
        }
        self.lobbies.clear();
        self.status = "ИЩУ КОМНАТЫ...".to_string();
        if let Some(s) = &self.steam {
            self.busy = true;
            s.request_list();
        }
        self.udp = UdpSocket::bind("0.0.0.0:0").ok().filter(|s| s.set_nonblocking(true).is_ok() && s.set_broadcast(true).is_ok());
        self.scan_t = 1.6;
        self.scan_ping_t = 0.0;
    }

    /// Вопрос "есть тут комнаты?" всем в локальной сети.
    fn scan_ping(&self) {
        if let Some(s) = &self.udp {
            let _ = s.send_to(ASK, (Ipv4Addr::BROADCAST, PORT));
            // Общая рассылка уходит только с одного сетевого адаптера, поэтому отдельно
            // спрашиваем свою подсеть (в домашних сетях она почти всегда /24).
            if let Some(ip) = local_ip() {
                let o = ip.octets();
                let _ = s.send_to(ASK, (Ipv4Addr::new(o[0], o[1], o[2], 255), PORT));
            }
        }
    }

    pub fn join(&mut self, room: RoomAddr) {
        self.ensure();
        if self.busy {
            return;
        }
        if self.online() {
            self.leave();
        }
        match room {
            RoomAddr::Steam(id) => {
                if let Some(s) = &self.steam {
                    self.busy = true;
                    self.status = "ПОДКЛЮЧАЮСЬ...".to_string();
                    s.join(id);
                }
            }
            RoomAddr::Lan(addr) => {
                self.busy = true;
                self.status = format!("ПОДКЛЮЧАЮСЬ К {}...", addr.ip());
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let r = TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5));
                    let _ = tx.send(r.map_err(|e| format!("{:?}", e.kind())));
                });
            }
        }
    }

    /// Вход по набранному адресу. false - адрес не разобрать.
    pub fn join_ip(&mut self, text: &str) -> bool {
        match parse_addr(text) {
            Some(addr) => {
                self.join(RoomAddr::Lan(addr));
                true
            }
            None => {
                self.status = "НЕПОНЯТНЫЙ АДРЕС. ПРИМЕР: 192.168.1.5".to_string();
                false
            }
        }
    }

    pub fn leave(&mut self) {
        let bye = Wr::new(T_BYE).0;
        match self.mode {
            Mode::Host => self.broadcast(&bye, true),
            Mode::Client => {
                if let Some(h) = self.host {
                    self.send(h, &bye, true);
                }
            }
            Mode::Offline => {}
        }
        if let Some(s) = &mut self.steam {
            s.leave_lobby();
        }
        self.conns.borrow_mut().clear();
        self.listener = None;
        self.udp = None;
        self.scan_t = 0.0;
        self.lan_ip = None;
        self.mode = Mode::Offline;
        self.host = None;
        self.peers.clear();
        self.synced = false;
        self.ping_ms = None;
        self.status = "НЕ В СЕТИ".to_string();
    }

    fn send(&self, to: PeerId, data: &[u8], reliable: bool) {
        if let Some(c) = self.conns.borrow_mut().iter_mut().find(|c| c.id == to && !c.dead) {
            c.queue(data);
        } else if let Some(s) = &self.steam {
            s.send(to, data, reliable);
        }
    }

    fn broadcast(&self, data: &[u8], reliable: bool) {
        for p in &self.peers {
            self.send(p.id, data, reliable);
        }
    }

    pub fn send_cmd(&self, c: &Cmd) {
        if let (Mode::Client, Some(h)) = (self.mode, self.host) {
            self.send(h, &enc_cmd(c), true);
        }
    }

    pub fn broadcast_op(&self, o: &Op) {
        if self.mode == Mode::Host {
            self.broadcast(&enc_op(o), true);
        }
    }

    /// Хост: разослать всем новый снимок мира (после нового витка карта пустая).
    pub fn broadcast_world(&self, text: &str) {
        if self.mode == Mode::Host {
            self.broadcast(&Wr::new(T_WORLD).str(text).0, true);
        }
    }

    /// Отмечает достижение в Steam, если клиент подключён. В тестовом приложении Spacewar
    /// наших достижений нет - тогда Steam просто отказывает, игра показывает достижение сама.
    pub fn achievement(&self, id: &str) -> bool {
        self.steam.as_ref().map_or(false, |s| s.achievement(id))
    }

    pub fn send_world(&self, to: PeerId, text: &str) {
        self.send(to, &Wr::new(T_WORLD).str(text).0, true);
    }

    /// Хосту: пора ли разослать состояние экономики (3 раза в секунду).
    pub fn state_due(&mut self) -> bool {
        if self.mode == Mode::Host && !self.peers.is_empty() && self.state_t <= 0.0 {
            self.state_t = 0.33;
            return true;
        }
        false
    }

    pub fn broadcast_state(&self, s: &StateMsg) {
        self.broadcast(&enc_state(s), true);
    }

    pub fn broadcast_logic(&self, ticks: u64, st: &[(bool, u32, bool)]) {
        if !st.is_empty() {
            self.broadcast(&enc_logic(ticks, st), true);
        }
    }

    /// Подставной игрок для скриншота-самопроверки (в сеть ничего не уходит).
    pub fn fake_peer(&mut self, id: PeerId, name: &str, p: Presence) {
        self.peers.push(Peer { id, name: name.to_string(), p, silence: 0.0, ping_ms: None });
    }

    /// Пинг для счётчика: клиенту - до хоста, хосту - до самого медленного игрока.
    pub fn ping(&self) -> Option<f32> {
        match self.mode {
            Mode::Client => self.ping_ms,
            Mode::Host => self.peers.iter().filter_map(|p| p.ping_ms).reduce(f32::max),
            Mode::Offline => None,
        }
    }

    fn now_us(&self) -> u64 {
        self.clock.elapsed().as_micros() as u64
    }

    pub fn set_presence(&mut self, p: Presence) {
        self.cursor = p;
    }

    fn handle(&mut self, from: PeerId, data: &[u8], out: &mut Vec<NetEvent>) {
        let mut r = Rd(data);
        let tag = match r.u8() {
            Some(t) => t,
            None => return,
        };
        // Пинг одинаков у хоста и клиента: на T_PING отвечаем сразу, по T_PONG считаем задержку.
        let known = match self.mode {
            Mode::Host => self.peers.iter().any(|p| p.id == from),
            Mode::Client => Some(from) == self.host,
            Mode::Offline => false,
        };
        if known && (tag == T_PING || tag == T_PONG) {
            if let Some(t) = r.u64() {
                if tag == T_PING {
                    self.send(from, &Wr::new(T_PONG).u64(t).0, false);
                } else {
                    let rtt = self.now_us().saturating_sub(t) as f32 / 1000.0;
                    match self.mode {
                        Mode::Host => {
                            if let Some(p) = self.peers.iter_mut().find(|p| p.id == from) {
                                p.ping_ms = Some(smooth_ping(p.ping_ms, rtt));
                            }
                        }
                        _ => self.ping_ms = Some(smooth_ping(self.ping_ms, rtt)),
                    }
                }
            }
            return;
        }
        match self.mode {
            Mode::Host => {
                if tag == T_HELLO {
                    let name = r.str().unwrap_or_default();
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == from) {
                        p.silence = 0.0;
                    } else if self.peers.len() < 7 {
                        self.peers.push(Peer { id: from, name: name.clone(), p: Presence::default(), silence: 0.0, ping_ms: None });
                        out.push(NetEvent::Info(format!("{} ПОДКЛЮЧИЛСЯ", name)));
                    } else {
                        return;
                    }
                    out.push(NetEvent::Hello(from, name));
                    return;
                }
                let idx = match self.peers.iter().position(|p| p.id == from) {
                    Some(i) => i,
                    None => return,
                };
                self.peers[idx].silence = 0.0;
                match tag {
                    T_CMD => {
                        if let Some(c) = dec_cmd(&mut r) {
                            out.push(NetEvent::Cmd(c));
                        }
                    }
                    T_CURSOR => {
                        if let Some(p) = rd_pres(&mut r) {
                            self.peers[idx].p = p;
                        }
                    }
                    T_BYE => {
                        let p = self.peers.remove(idx);
                        self.conns.borrow_mut().retain(|c| c.id != p.id);
                        out.push(NetEvent::Info(format!("{} ОТКЛЮЧИЛСЯ", p.name)));
                    }
                    _ => {}
                }
            }
            Mode::Client => {
                if Some(from) != self.host {
                    return;
                }
                self.host_silence = 0.0;
                match tag {
                    T_WORLD => {
                        if let Some(s) = r.str() {
                            self.synced = true;
                            self.status = "В ЧУЖОЙ КОМНАТЕ".to_string();
                            out.push(NetEvent::World(s));
                        }
                    }
                    T_OP if self.synced => {
                        if let Some(o) = dec_op(&mut r) {
                            out.push(NetEvent::Op(o));
                        }
                    }
                    T_STATE if self.synced => {
                        if let Some(st) = dec_state(&mut r) {
                            out.push(NetEvent::State(st));
                        }
                    }
                    T_LOGIC if self.synced => {
                        if let Some((ticks, st)) = dec_logic(&mut r) {
                            out.push(NetEvent::Logic(ticks, st));
                        }
                    }
                    T_CURSORS => {
                        self.peers.clear();
                        while let (Some(id), Some(p), Some(name)) = (r.u64(), rd_pres(&mut r), r.str()) {
                            if id != self.my_id && self.peers.len() < 8 {
                                self.peers.push(Peer { id, name, p, silence: 0.0, ping_ms: None });
                            }
                        }
                    }
                    T_BYE => {
                        self.leave();
                        out.push(NetEvent::Lost("ХОСТ ЗАКРЫЛ КОМНАТУ".to_string()));
                    }
                    _ => {}
                }
            }
            Mode::Offline => {}
        }
    }

    fn poll_steam(&mut self, msgs: &mut Vec<(PeerId, Vec<u8>)>) {
        let mut evs = Vec::new();
        match &mut self.steam {
            Some(s) => s.poll(&mut evs, msgs),
            None => return,
        }
        for ev in evs {
            match ev {
                SteamEv::Created(Ok(())) => {
                    self.busy = false;
                    self.become_host();
                }
                SteamEv::Created(Err(e)) => {
                    self.busy = false;
                    if self.listener.is_some() {
                        // Лобби Steam не создалось, но в локальной сети комната открыта.
                        self.become_host();
                    } else {
                        self.status = format!("НЕ УДАЛОСЬ СОЗДАТЬ: {}", e).to_uppercase();
                    }
                }
                SteamEv::Joined(Ok(owner)) => {
                    self.busy = false;
                    self.start_client(Some(owner));
                }
                SteamEv::Joined(Err(())) => {
                    self.busy = false;
                    self.status = "НЕ УДАЛОСЬ ВОЙТИ В КОМНАТУ".to_string();
                }
                SteamEv::List(list) => {
                    self.busy = false;
                    for (id, name, members) in list {
                        self.lobbies.push(LobbyInfo { addr: RoomAddr::Steam(id), name: format!("{} - STEAM", name), members });
                    }
                    self.status = format!("НАЙДЕНО КОМНАТ: {}", self.lobbies.len());
                }
                SteamEv::JoinRequested(l) => self.join(RoomAddr::Steam(l)),
            }
        }
    }

    fn start_client(&mut self, host: Option<PeerId>) {
        self.host = host;
        self.mode = Mode::Client;
        self.synced = false;
        self.hello_t = 0.0;
        self.host_silence = 0.0;
        self.peers.clear();
        self.status = "ЖДУ МИР ОТ ХОСТА...".to_string();
    }

    /// Прямые соединения: новые игроки, поиск комнат, входящие сообщения.
    fn poll_lan(&mut self, dt: f32, msgs: &mut Vec<(PeerId, Vec<u8>)>, out: &mut Vec<NetEvent>) {
        // Итог подключения к комнате по адресу.
        while let Ok(r) = self.rx.try_recv() {
            self.busy = false;
            match r.ok().and_then(|s| Conn::new(s, self.my_id)) {
                Some(c) if !self.online() => {
                    self.conns.borrow_mut().push(c);
                    self.udp = None;
                    self.scan_t = 0.0;
                    self.start_client(None);
                }
                Some(_) => {}
                None => self.status = "НЕ УДАЛОСЬ ПОДКЛЮЧИТЬСЯ: КОМНАТА НЕ ОТВЕЧАЕТ".to_string(),
            }
        }
        // Хост: принять тех, кто постучался, и ответить ищущим комнаты.
        if self.mode == Mode::Host {
            if let Some(l) = &self.listener {
                while let Ok((s, _)) = l.accept() {
                    if let Some(c) = Conn::new(s, self.my_id) {
                        self.conns.borrow_mut().push(c);
                    }
                }
            }
            if let Some(s) = &self.udp {
                let mut buf = [0u8; 64];
                while let Ok((n, from)) = s.recv_from(&mut buf) {
                    if &buf[..n] == ASK {
                        let mut reply = ANSWER.to_vec();
                        reply.push(self.peers.len() as u8 + 1);
                        reply.extend_from_slice(self.my_name.as_bytes());
                        let _ = s.send_to(&reply, from);
                    }
                }
            }
        }
        // Поиск комнат: вопрос повторяется, ответы копятся в списке.
        if self.scan_t > 0.0 {
            self.scan_ping_t -= dt;
            if self.scan_ping_t <= 0.0 {
                self.scan_ping_t = 0.5;
                self.scan_ping();
            }
            if let Some(s) = &self.udp {
                let mut buf = [0u8; 256];
                while let Ok((n, from)) = s.recv_from(&mut buf) {
                    let addr = RoomAddr::Lan(SocketAddr::new(from.ip(), PORT));
                    if n > ANSWER.len() && buf[..n].starts_with(ANSWER) && !self.lobbies.iter().any(|l| l.addr == addr) {
                        let name = String::from_utf8_lossy(&buf[ANSWER.len() + 1..n]).into_owned();
                        self.lobbies.push(LobbyInfo { addr, name: format!("{} - ЛОКАЛЬНАЯ СЕТЬ", name), members: buf[ANSWER.len()] as usize });
                    }
                }
            }
            self.scan_t -= dt;
            if self.scan_t <= 0.0 {
                self.udp = None;
                if !self.busy && !self.online() {
                    self.status = format!("НАЙДЕНО КОМНАТ: {}", self.lobbies.len());
                }
            }
        }
        // Входящие сообщения. Первое в каждом соединении - приветствие с номером игрока.
        let mut lost = Vec::new();
        {
            let mut conns = self.conns.borrow_mut();
            let mut frames = Vec::new();
            for i in 0..conns.len() {
                conns[i].age += dt;
                frames.clear();
                conns[i].read(&mut frames);
                for f in frames.drain(..) {
                    if conns[i].id != 0 {
                        msgs.push((conns[i].id, f));
                        continue;
                    }
                    let id = (f.len() == 16 && f.starts_with(MAGIC)).then(|| u64::from_le_bytes(f[8..].try_into().unwrap()));
                    match id {
                        Some(id) if id != 0 && id != self.my_id && !conns.iter().any(|c| c.id == id) => {
                            conns[i].id = id;
                            if self.mode == Mode::Client && self.host.is_none() {
                                self.host = Some(id);
                            }
                        }
                        // Не наша игра или другая версия протокола.
                        _ => conns[i].dead = true,
                    }
                }
                conns[i].flush();
                // Молчит после соединения или уже не игрок: держать незачем.
                let stale = if conns[i].id == 0 { conns[i].age > 5.0 } else { self.mode == Mode::Host && conns[i].age > TIMEOUT && !self.peers.iter().any(|p| p.id == conns[i].id) };
                if stale {
                    conns[i].dead = true;
                }
                if conns[i].dead {
                    lost.push(conns[i].id);
                }
            }
            conns.retain(|c| !c.dead);
        }
        for id in lost {
            match self.mode {
                Mode::Host => {
                    if let Some(i) = self.peers.iter().position(|p| p.id == id) {
                        let p = self.peers.remove(i);
                        out.push(NetEvent::Info(format!("{} ОТКЛЮЧИЛСЯ", p.name)));
                    }
                }
                Mode::Client if self.conns.borrow().is_empty() && (self.host.is_none() || self.host == Some(id)) => {
                    let why = if self.synced { "СВЯЗЬ С ХОСТОМ ПОТЕРЯНА" } else { "НЕ УДАЛОСЬ ВОЙТИ: КОМНАТА ЗАКРЫТА ИЛИ ДРУГАЯ ВЕРСИЯ ИГРЫ" };
                    self.leave();
                    out.push(NetEvent::Lost(why.to_string()));
                }
                _ => {}
            }
        }
    }

    /// Вызывается каждый кадр: колбэки Steam, прямые соединения, входящие сообщения, таймеры.
    pub fn poll(&mut self, dt: f32) -> Vec<NetEvent> {
        let mut out = Vec::new();
        let mut msgs = Vec::new();
        self.poll_steam(&mut msgs);
        self.poll_lan(dt, &mut msgs, &mut out);
        for (from, data) in msgs {
            self.handle(from, &data, &mut out);
        }

        self.cursor_t -= dt;
        self.state_t -= dt;
        self.ping_t -= dt;
        if self.ping_t <= 0.0 {
            self.ping_t = PING_EVERY;
            let ping = Wr::new(T_PING).u64(self.now_us()).0;
            match (self.mode, self.host) {
                (Mode::Host, _) => self.broadcast(&ping, false),
                (Mode::Client, Some(h)) if self.synced => self.send(h, &ping, false),
                _ => {}
            }
        }
        match self.mode {
            Mode::Host => {
                for p in &mut self.peers {
                    p.silence += dt;
                }
                let before = self.peers.len();
                self.peers.retain(|p| p.silence < TIMEOUT);
                if self.peers.len() != before {
                    out.push(NetEvent::Info("ИГРОК ОТВАЛИЛСЯ ПО ТАЙМАУТУ".to_string()));
                }
                if self.cursor_t <= 0.0 && !self.peers.is_empty() {
                    self.cursor_t = 0.15;
                    let mut w = wr_pres(Wr::new(T_CURSORS).u64(self.my_id), &self.cursor).str(&self.my_name);
                    for p in &self.peers {
                        w = wr_pres(w.u64(p.id), &p.p).str(&p.name);
                    }
                    self.broadcast(&w.0, false);
                }
            }
            Mode::Client => {
                self.host_silence += dt;
                if let Some(h) = self.host {
                    if !self.synced {
                        self.hello_t -= dt;
                        if self.hello_t <= 0.0 {
                            self.hello_t = 1.0;
                            self.send(h, &Wr::new(T_HELLO).str(&self.my_name).0, true);
                        }
                    } else if self.cursor_t <= 0.0 {
                        self.cursor_t = 0.15;
                        self.send(h, &wr_pres(Wr::new(T_CURSOR), &self.cursor).0, false);
                    }
                }
                if self.host_silence > TIMEOUT {
                    self.leave();
                    out.push(NetEvent::Lost("СВЯЗЬ С ХОСТОМ ПОТЕРЯНА".to_string()));
                }
            }
            Mode::Offline => {}
        }
        out
    }
}

// ---------- упаковка сообщений ----------

const T_HELLO: u8 = 1;
const T_WORLD: u8 = 2;
const T_CMD: u8 = 3;
const T_OP: u8 = 4;
const T_STATE: u8 = 5;
const T_CURSOR: u8 = 6;
const T_CURSORS: u8 = 7;
const T_BYE: u8 = 8;
/// Замер пинга: отправитель кладёт своё время, получатель возвращает его как есть.
const T_PING: u8 = 9;
const T_PONG: u8 = 10;
const T_LOGIC: u8 = 11;

/// Состояние слоя сигналов: номер такта, число деталей и по 5 байт на деталь.
pub fn enc_logic(ticks: u64, st: &[(bool, u32, bool)]) -> Vec<u8> {
    let mut w = Wr::new(T_LOGIC).u64(ticks).u32(st.len() as u32);
    for s in st {
        w = w.u8(s.0 as u8 | (s.2 as u8) << 1).u32(s.1);
    }
    w.0
}

fn dec_logic(r: &mut Rd) -> Option<(u64, Vec<(bool, u32, bool)>)> {
    let ticks = r.u64()?;
    let n = r.u32()? as usize;
    let mut v = Vec::new();
    for _ in 0..n.min(100_000) {
        let f = r.u8()?;
        v.push((f & 1 == 1, r.u32()?, f & 2 == 2));
    }
    Some((ticks, v))
}

/// Состояние экономики хоста одним сообщением.
pub fn enc_state(s: &StateMsg) -> Vec<u8> {
    let mut w = Wr::new(T_STATE).u64(s.money.to_bits()).u32(s.level).u32(s.delivered);
    for u in s.upgrades {
        w = w.u8(u);
    }
    w = w.u32(s.income.to_bits()).u64(s.time.to_bits());
    let mut bits = vec![0u8; (N_ALL_IDS + 7) / 8];
    for (i, d) in s.discovered.iter().enumerate().take(N_ALL_IDS) {
        if *d {
            bits[i / 8] |= 1 << (i % 8);
        }
    }
    w.0.extend_from_slice(&bits);
    w.str(&s.extra).0
}

fn dec_state(r: &mut Rd) -> Option<StateMsg> {
    let money = f64::from_bits(r.u64()?);
    let (level, delivered) = (r.u32()?, r.u32()?);
    let upgrades = [r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?];
    let income = f32::from_bits(r.u32()?);
    let time = f64::from_bits(r.u64()?);
    let bits = r.take((N_ALL_IDS + 7) / 8)?;
    let discovered = (0..N_ALL_IDS).map(|i| bits[i / 8] >> (i % 8) & 1 == 1).collect();
    let extra = r.str()?;
    Some(StateMsg { money, level, delivered, upgrades, income, time, discovered, extra })
}

struct Wr(Vec<u8>);

impl Wr {
    fn new(tag: u8) -> Wr {
        Wr(vec![tag])
    }
    fn u8(mut self, v: u8) -> Wr {
        self.0.push(v);
        self
    }
    fn i16(mut self, v: i32) -> Wr {
        self.0.extend_from_slice(&(v as i16).to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Wr {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(mut self, v: u64) -> Wr {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(mut self, s: &str) -> Wr {
        let b = &s.as_bytes()[..s.len().min(60000)];
        self.0.extend_from_slice(&(b.len() as u32).to_le_bytes());
        self.0.extend_from_slice(b);
        self
    }
}

struct Rd<'a>(&'a [u8]);

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Some(a)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn i16(&mut self) -> Option<i32> {
        self.take(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as i32)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        self.take(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }
}

fn enc_cmd(c: &Cmd) -> Vec<u8> {
    let w = Wr::new(T_CMD);
    match *c {
        Cmd::Place(k, x, y, d) => w.u8(0).u8(k.0).i16(x).i16(y).u8(d as u8),
        Cmd::Remove(x, y) => w.u8(1).i16(x).i16(y),
        Cmd::BeltDir(x, y, d) => w.u8(2).i16(x).i16(y).u8(d as u8),
        Cmd::Upgrade(i) => w.u8(3).u8(i),
        Cmd::LevelUp(x, y) => w.u8(4).i16(x).i16(y),
        Cmd::SetPorts(x, y, p) => w.u8(5).i16(x).i16(y).u8(p),
        Cmd::FilterReset(x, y) => w.u8(6).i16(x).i16(y),
        Cmd::SetCfg(x, y, v) => w.u8(7).i16(x).i16(y).u64(v),
        Cmd::LRemove(x, y) => w.u8(8).i16(x).i16(y),
        Cmd::LCfg(x, y, v) => w.u8(9).i16(x).i16(y).u64(v),
        Cmd::Order(i) => w.u8(10).u8(i),
        Cmd::Skin(i) => w.u8(11).u8(i),
        Cmd::Prestige => w.u8(12),
    }
    .0
}

fn dec_cmd(r: &mut Rd) -> Option<Cmd> {
    Some(match r.u8()? {
        0 => {
            let k = Kind::from_u8(r.u8()?)?;
            Cmd::Place(k, r.i16()?, r.i16()?, Dir::from_u8(r.u8()?))
        }
        1 => Cmd::Remove(r.i16()?, r.i16()?),
        2 => Cmd::BeltDir(r.i16()?, r.i16()?, Dir::from_u8(r.u8()?)),
        3 => Cmd::Upgrade(r.u8()?),
        4 => Cmd::LevelUp(r.i16()?, r.i16()?),
        5 => Cmd::SetPorts(r.i16()?, r.i16()?, r.u8()?),
        6 => Cmd::FilterReset(r.i16()?, r.i16()?),
        7 => Cmd::SetCfg(r.i16()?, r.i16()?, r.u64()?),
        8 => Cmd::LRemove(r.i16()?, r.i16()?),
        9 => Cmd::LCfg(r.i16()?, r.i16()?, r.u64()?),
        10 => Cmd::Order(r.u8()?),
        11 => Cmd::Skin(r.u8()?),
        12 => Cmd::Prestige,
        _ => return None,
    })
}

fn enc_op(o: &Op) -> Vec<u8> {
    let w = Wr::new(T_OP);
    match *o {
        Op::Set(x, y, k, d) => w.u8(0).i16(x).i16(y).u8(k.0).u8(d as u8),
        Op::Clear(x, y) => w.u8(1).i16(x).i16(y),
        Op::Dir(x, y, d) => w.u8(2).i16(x).i16(y).u8(d as u8),
        Op::Level(x, y, l) => w.u8(3).i16(x).i16(y).u8(l),
        Op::Ports(x, y, p) => w.u8(4).i16(x).i16(y).u8(p),
        Op::FilterReset(x, y) => w.u8(5).i16(x).i16(y),
        Op::Cfg(x, y, v) => w.u8(6).i16(x).i16(y).u64(v),
        Op::LClear(x, y) => w.u8(7).i16(x).i16(y),
        Op::LCfg(x, y, v) => w.u8(8).i16(x).i16(y).u64(v),
    }
    .0
}

fn dec_op(r: &mut Rd) -> Option<Op> {
    Some(match r.u8()? {
        0 => {
            let (x, y) = (r.i16()?, r.i16()?);
            Op::Set(x, y, Kind::from_u8(r.u8()?)?, Dir::from_u8(r.u8()?))
        }
        1 => Op::Clear(r.i16()?, r.i16()?),
        2 => Op::Dir(r.i16()?, r.i16()?, Dir::from_u8(r.u8()?)),
        3 => Op::Level(r.i16()?, r.i16()?, r.u8()?),
        4 => Op::Ports(r.i16()?, r.i16()?, r.u8()?),
        5 => Op::FilterReset(r.i16()?, r.i16()?),
        6 => Op::Cfg(r.i16()?, r.i16()?, r.u64()?),
        7 => Op::LClear(r.i16()?, r.i16()?),
        8 => Op::LCfg(r.i16()?, r.i16()?, r.u64()?),
        _ => return None,
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    /// Хост по ответу на пинг считает задержку до игрока; чужие ответы не учитывает.
    #[test]
    fn ping_from_pong() {
        let mut net = Net::new();
        net.mode = Mode::Host;
        net.fake_peer(5, "Drug", Presence::default());
        assert_eq!(net.ping(), None);
        std::thread::sleep(std::time::Duration::from_millis(30));
        let pong = Wr::new(T_PONG).u64(0).0;
        let mut out = Vec::new();
        net.handle(99, &pong, &mut out);
        assert_eq!(net.ping(), None);
        net.handle(5, &pong, &mut out);
        let p = net.ping().expect("пинг посчитан");
        assert!(p >= 30.0 && p < 5000.0, "пинг {}", p);
        assert!(out.is_empty());
        // Сглаживание: резкий скачок входит не целиком.
        assert!((smooth_ping(Some(100.0), 200.0) - 135.0).abs() < 0.01);
        net.leave();
        assert_eq!(net.ping(), None);
    }

    /// Хост и клиент после 5 минут симуляции совпадают по деньгам, ценам рынка, гильдиям,
    /// мегапроекту и состоянию сигналов. Сообщения идут через настоящие кодеки и разбор клиента.
    #[test]
    fn host_and_client_agree_after_five_minutes() {
        use crate::world::{Item, HUB_X, HUB_Y, W};
        let k = Kind::by_name;
        let mut host = World::new(3);
        host.money = 1e6;
        host.level = 12;
        // Две линии в лавку, на одной - ворота по таймеру и счётчик предметов.
        for (y, e) in [(HUB_Y, 1u8), (HUB_Y + 2, 2)] {
            host.veins[(y * W + HUB_X - 4) as usize] = e;
            host.place(Kind(1), HUB_X - 4, y, Dir::E).unwrap();
            for x in HUB_X - 3..HUB_X {
                host.place(Kind(0), x, y, Dir::E).unwrap();
            }
        }
        let (x, y) = (HUB_X - 2, HUB_Y);
        host.place(k("ИСПОЛНИТЕЛЬ"), x, y, Dir::N).unwrap();
        host.place(k("ПРОВОД"), x, y - 1, Dir::N).unwrap();
        host.place(k("ТАЙМЕР"), x, y - 2, Dir::S).unwrap();
        host.set_lparam(x, y - 2, 70);
        host.place(k("ДАТЧИК ПРЕДМЕТА"), x + 1, y, Dir::N).unwrap();
        host.place(k("СЧЁТЧИК"), x + 1, y - 1, Dir::N).unwrap();
        host.set_lparam(x + 1, y - 1, 5);
        host.place(k("ЗАЩЁЛКА"), x + 1, y - 2, Dir::N).unwrap();
        let frame = |w: &mut World| w.update(1.0 / 60.0);
        (0..1200).for_each(|_| frame(&mut host));
        host.take_order(0).unwrap();

        // Клиент входит через 20 секунд: получает снимок мира, дальше считает сам и слушает хоста.
        let mut client = World::load(&host.save()).expect("снимок читается");
        client.replica = true;
        let mut net = Net::new();
        net.mode = Mode::Client;
        net.host = Some(1);
        net.synced = true;
        // Внеочередная рассылка логики: хост шлёт её, когда сработал датчик (не чаще 10 раз в секунду).
        let push_logic = |host: &World, client: &mut World, net: &mut Net| {
            let mut evs = Vec::new();
            net.handle(1, &enc_logic(host.logic_ticks, &host.logic_state()), &mut evs);
            match evs.pop() {
                Some(NetEvent::Logic(t, st)) => client.apply_logic_state(t, &st),
                _ => panic!("состояние логики не разобрано"),
            }
        };
        let sync = |host: &World, client: &mut World, net: &mut Net| {
            let st = StateMsg {
                money: host.money,
                level: host.level as u32,
                delivered: host.delivered,
                upgrades: host.upgrades,
                income: 0.0,
                time: host.time,
                discovered: host.discovered.clone(),
                extra: host.econ_save(),
            };
            let mut evs = Vec::new();
            net.handle(1, &enc_state(&st), &mut evs);
            net.handle(1, &enc_logic(host.logic_ticks, &host.logic_state()), &mut evs);
            assert_eq!(evs.len(), 2, "оба сообщения разобраны");
            for ev in evs {
                match ev {
                    NetEvent::State(s) => {
                        client.apply_state(s.money, s.level as usize, s.delivered, s.upgrades, &s.discovered);
                        client.sync_time(s.time);
                        s.extra.lines().for_each(|l| {
                            client.econ_line(l);
                        });
                    }
                    NetEvent::Logic(t, st) => client.apply_logic_state(t, &st),
                    _ => panic!("неожиданное событие"),
                }
            }
        };
        // Снимок мира несёт и память деталей: клиент входит с теми же таймером и счётчиком.
        assert_eq!(client.logic_state(), host.logic_state());
        let (mut drift, mut off_frames, mut pushes, mut since) = (0, 0, 0, 0);
        for f in 0..5 * 60 * 60 {
            frame(&mut host);
            frame(&mut client);
            since += 1;
            if f % 20 == 19 {
                drift += (client.logic_state() != host.logic_state()) as u32;
                sync(&host, &mut client, &mut net);
                host.logic_dirty = false;
                since = 0;
            } else if host.logic_dirty && since >= 6 {
                push_logic(&host, &mut client, &mut net);
                host.logic_dirty = false;
                pushes += 1;
                since = 0;
            }
            off_frames += (client.logic_state() != host.logic_state()) as u32;
        }
        assert!(host.total_sold > 300 && host.logic_ticks >= 19_000, "хост торговал и считал логику");
        assert_eq!(client.money, host.money);
        assert_eq!((client.level, client.delivered, client.rep, client.order.map(|o| o.1)), (host.level, host.delivered, host.rep, host.order.map(|o| o.1)));
        assert_eq!(client.logic_state(), host.logic_state());
        assert_eq!(client.logic_ticks, host.logic_ticks);
        assert_eq!((client.great.clone(), client.discovered.clone()), (host.great.clone(), host.discovered.clone()));
        for it in [Item::base(0), Item::base(1), Item::new(1, 2, 0)] {
            let (a, b) = (client.market_factor(it), host.market_factor(it));
            assert!((a - b).abs() < 1e-3, "цена рынка {}: {} и {}", it.name(), a, b);
        }
        assert!(host.market_factor(Item::base(1)) < 0.9, "рынок действительно насыщен");
        // У копии мира свои предметы на лентах, поэтому показания датчиков она берёт у хоста,
        // а всё остальное (таймеры, вентили, счётчики) досчитывает сама и не расходится.
        println!(
            "СЕТЬ: за 5 минут 900 сверок, расхождений логики до сверки: {}; кадров с расхождением {} из 18000; внеочередных рассылок {}",
            drift, off_frames, pushes
        );
        assert!(drift <= 5 && off_frames <= 400, "логика клиента почти не отстаёт: {} и {}", drift, off_frames);
    }

    #[test]
    fn codec_roundtrip() {
        let cmds = [
            Cmd::Place(Kind(17), 79, 3, Dir::W),
            // Клетка подземелья: строки второго слоя лежат дальше строк поверхности.
            Cmd::Place(Kind(62), 79, crate::world::ROWS - 1, Dir::S),
            Cmd::Remove(0, crate::world::DUNGEON_Y),
            Cmd::Remove(12, 40),
            Cmd::BeltDir(5, 6, Dir::S),
            Cmd::Upgrade(3),
            Cmd::LevelUp(44, 36),
            Cmd::SetPorts(3, 4, 0b10_01_11_00),
            Cmd::FilterReset(7, 8),
            Cmd::SetCfg(9, 10, 0x1F_FFFF_FFFF),
            Cmd::LRemove(1, 2),
            Cmd::LCfg(3, 4, 9999),
            Cmd::Order(2),
            Cmd::Skin(4),
            Cmd::Prestige,
        ];
        for c in cmds {
            let bytes = enc_cmd(&c);
            let back = dec_cmd(&mut Rd(&bytes[1..])).expect("decodes");
            assert_eq!(format!("{:?}", c), format!("{:?}", back));
        }
        let ops = [Op::Set(1, 2, Kind(31), Dir::E), Op::Set(79, crate::world::ROWS - 1, Kind(62), Dir::N), Op::Clear(70, 71), Op::Dir(9, 9, Dir::N), Op::Level(8, 7, 4), Op::Ports(1, 1, 200), Op::FilterReset(2, 2), Op::Cfg(3, 3, u64::MAX), Op::LClear(4, 4), Op::LCfg(5, 5, 77)];
        for o in ops {
            let bytes = enc_op(&o);
            let back = dec_op(&mut Rd(&bytes[1..])).expect("decodes");
            assert_eq!(format!("{:?}", o), format!("{:?}", back));
        }
        // Обрезанные и мусорные сообщения не должны ронять разбор.
        assert!(dec_cmd(&mut Rd(&[0, 200])).is_none());
        assert!(dec_op(&mut Rd(&[0, 1])).is_none());
        assert!(Rd(&[255, 255, 255, 255, 1]).str().is_none());
    }

    /// Клиент, повторяющий за хостом снимок мира и операции, получает тот же набор построек.
    #[test]
    fn replica_follows_host() {
        let mut host = World::new(7);
        host.money = 1e6;
        host.level = 30;
        host.place(Kind(0), 10, 10, Dir::E).unwrap();
        let mut client = World::load(&host.save()).expect("snapshot loads");
        client.replica = true;

        let mut ops = Vec::new();
        for (k, x, y, d) in [(0u8, 11, 10, Dir::E), (3, 12, 10, Dir::E), (35, 13, 10, Dir::S), (0, 10, 10, Dir::S)] {
            if host.place(Kind(k), x, y, d) == Ok(true) {
                ops.push(Op::Set(x, y, Kind(k), d));
            }
        }
        assert!(host.remove(11, 10));
        ops.push(Op::Clear(11, 10));
        for o in ops {
            let bytes = enc_op(&o);
            match dec_op(&mut Rd(&bytes[1..])).unwrap() {
                Op::Set(x, y, k, d) => client.force_set(k, x, y, d),
                Op::Clear(x, y) => client.force_clear(x, y),
                Op::Dir(x, y, d) => client.set_belt_dir(x, y, d),
                Op::Level(x, y, l) => client.force_level(x, y, l),
                Op::Ports(x, y, p) => {
                    client.set_ports(x, y, p);
                }
                Op::FilterReset(x, y) => {
                    client.filter_reset(x, y);
                }
                Op::Cfg(x, y, v) => {
                    client.set_cfg(x, y, v);
                }
                Op::LClear(x, y) => client.force_clear_logic(x, y),
                Op::LCfg(x, y, v) => {
                    client.set_lparam(x, y, v);
                }
            }
        }
        let tiles = |w: &World| w.save().lines().skip(3).map(String::from).collect::<Vec<_>>();
        assert_eq!(tiles(&host), tiles(&client));

        // Продажи на копии не трогают экономику - её задаёт только хост.
        let before = client.money;
        for _ in 0..600 {
            client.update(0.05);
        }
        assert_eq!(client.money, before);
        client.apply_state(123.0, 31, 5, [1, 2, 3, 4, 5, 6, 7, 8], &host.discovered);
        assert_eq!((client.money, client.level, client.delivered), (123.0, 31, 5));
    }

    /// Сохранения прошлых версий (без строк D/U и без уровня машин) обязаны читаться.
    #[test]
    fn old_saves_load() {
        let v1 = "ALEMBIC1\n1234 4 17 2 1 0 3 99\n10 10 0 1\n11 10 2 1\n12 10 6 2\n";
        let w = World::load(v1).expect("v1 save loads");
        assert_eq!((w.money, w.level, w.delivered), (1234.0, 4, 17));
        assert_eq!(&w.upgrades[..], &[2, 1, 0, 3, 0, 0, 0, 0]);
        assert_eq!(w.get(11, 10).map(|b| (b.kind, b.level)), Some((Kind(2), 1)));
        assert_eq!(w.get(12, 10).map(|b| b.kind), Some(Kind(6)));

        // Новый формат: круг "сохранить - загрузить" ничего не теряет.
        let mut w = w;
        w.money = 1e9;
        w.upgrades[5] = 3;
        w.upgrades[7] = 10;
        assert_eq!(w.level_up(11, 10), Ok(2));
        assert_eq!(w.level_up(11, 10), Ok(3));
        assert!(w.level_up(10, 10).is_err(), "конвейер не прокачивается");
        let again = World::load(&w.save()).expect("v2 save loads");
        assert_eq!(again.save(), w.save());
        assert_eq!(again.get(11, 10).map(|b| b.level), Some(3));
        assert_eq!(again.upgrades, w.upgrades);
    }

    /// Машина третьего уровня работает вдвое быстрее первой, а снос возвращает всё потраченное.
    #[test]
    fn machine_levels() {
        let mut w = World::new(3);
        w.money = 1000.0;
        w.veins[(10 * crate::world::W + 10) as usize] = 1;
        w.place(Kind(1), 10, 10, Dir::E).unwrap();
        w.level_up(10, 10).unwrap();
        w.level_up(10, 10).unwrap();
        assert_eq!(w.money, 1000.0 - 20.0 - 40.0 - 80.0);
        assert_eq!(crate::world::level_mult(3), 2.0);
        assert!(w.remove(10, 10));
        assert_eq!(w.money, 1000.0);
    }

    #[test]
    fn contract_paths() {
        use crate::world::{contract, recipe_steps, Item, FINAL_LEVEL};
        // Простая эссенция - один шаг, пар - смеситель из двух стихий.
        assert_eq!(recipe_steps(Item::new(1, 1, 0)).len(), 1);
        let steam = recipe_steps(Item::new(3, 1, 0));
        assert_eq!((steam.len(), steam[0].machine, steam[0].inputs.len()), (1, "СМЕСИТЕЛЬ", 2));
        // У любого контракта путь заканчивается самим предметом и не содержит повторов.
        for lvl in 0..FINAL_LEVEL + 20 {
            let target = contract(lvl).item;
            let steps = recipe_steps(target);
            assert_eq!(steps.last().unwrap().out, target);
            for (i, s) in steps.iter().enumerate() {
                assert!(steps[..i].iter().all(|p| p.out != s.out));
                for inp in &s.inputs {
                    let base = inp.form == 0 && inp.purity == 1 && inp.mask.count_ones() == 1;
                    assert!(base || steps[..i].iter().any(|p| p.out == *inp), "вход сделан раньше");
                }
            }
        }
    }

    #[test]
    fn presence_roundtrip() {
        let p = Presence { x: 79, y: -3, tool: 17, dir: 2, mode: 2, rect: (4, 5, 40, 41) };
        let bytes = wr_pres(Wr::new(T_CURSOR), &p).0;
        assert_eq!(rd_pres(&mut Rd(&bytes[1..])), Some(p));
        assert!(rd_pres(&mut Rd(&bytes[1..bytes.len() - 1])).is_none(), "обрезанное сообщение отброшено");
        // В рассылке хоста записи идут подряд: идентификатор, состояние, имя.
        let list = wr_pres(Wr::new(T_CURSORS).u64(42), &p).str("Друг").0;
        let mut r = Rd(&list[1..]);
        assert_eq!((r.u64(), rd_pres(&mut r), r.str()), (Some(42), Some(p), Some("Друг".to_string())));
    }

    fn belt_len(w: &World, x: i32, y: i32) -> usize {
        w.get(x, y).map_or(0, |b| b.lanes[0].len())
    }

    /// Выход машины можно перенести на другую сторону, а сторону делителя - закрыть.
    #[test]
    fn configurable_sides() {
        use crate::world::{pack_ports, PORT_AUTO, PORT_OFF, PORT_OUT};
        let mut w = World::new(5);
        w.money = 1e6;
        w.level = 30;
        let (belt, ext, split) = (Kind(0), Kind(1), Kind(3));
        // Добыча смотрит на восток, но выход назначен направо (на юг).
        w.veins[(10 * crate::world::W + 10) as usize] = 1;
        w.place(ext, 10, 10, Dir::E).unwrap();
        w.place(belt, 11, 10, Dir::E).unwrap();
        w.place(belt, 10, 11, Dir::S).unwrap();
        assert!(w.set_ports(10, 10, pack_ports([PORT_AUTO, PORT_OUT, PORT_AUTO, PORT_AUTO])));
        // Делитель с закрытой левой стороной делит только вперёд и вправо.
        w.veins[(20 * crate::world::W + 10) as usize] = 2;
        w.place(ext, 10, 20, Dir::E).unwrap();
        w.place(split, 11, 20, Dir::E).unwrap();
        w.place(belt, 12, 20, Dir::E).unwrap();
        w.place(belt, 11, 19, Dir::N).unwrap();
        w.place(belt, 11, 21, Dir::S).unwrap();
        assert!(w.set_ports(11, 20, pack_ports([PORT_AUTO, PORT_AUTO, PORT_AUTO, PORT_OFF])));
        for _ in 0..400 {
            w.update(0.05);
        }
        assert_eq!((belt_len(&w, 11, 10), belt_len(&w, 10, 11)), (0, 2), "выход ушёл на юг");
        assert_eq!(belt_len(&w, 11, 19), 0, "закрытая сторона делителя пуста");
        assert!(belt_len(&w, 12, 20) > 0 && belt_len(&w, 11, 21) > 0);
        // Настройка сторон переживает сохранение, а строки без неё остаются в старом виде.
        let saved = w.save();
        assert!(saved.contains("11 20 3 1 1 192"));
        assert!(saved.contains("11 10 0 1 1\n"));
        let again = World::load(&saved).unwrap();
        assert_eq!(again.get(11, 20).map(|b| b.ports), w.get(11, 20).map(|b| b.ports));
        // Сторону можно переключать по кругу и вернуться к исходному.
        let mut p = 0;
        for _ in 0..4 {
            p = w.next_ports(12, 20, 0).unwrap_or(p);
        }
        assert!(w.next_ports(12, 20, 0).is_none(), "у конвейера сторон нет");
        let start = pack_ports(w.get(11, 20).unwrap().ports);
        let mut cur = start;
        for _ in 0..4 {
            cur = w.next_ports(11, 20, 1).unwrap();
            w.set_ports(11, 20, cur);
        }
        assert_eq!(cur, start);
    }
}
