//! Сетевая игра через Steam (тестовый AppID 480 "Spacewar"): лобби + ISteamNetworkingMessages.
//!
//! Схема: хост - единственный источник правды. Клиенты шлют команды строительства,
//! хост применяет их и рассылает всем результат и состояние экономики. Предметы на
//! лентах каждый клиент досчитывает сам - они только для вида.

use crate::world::{Dir, Kind, N_KINDS};
use std::sync::mpsc::{channel, Receiver, Sender};
use steamworks::networking_messages::SessionRequest;
use steamworks::networking_types::{NetworkingIdentity, SendFlags};
use steamworks::{
    CallbackHandle, Client, DistanceFilter, GameLobbyJoinRequested, LobbyId, LobbyKey, LobbyType, SteamId,
    StringFilter, StringFilterKind,
};

pub const APP_ID: u32 = 480;
/// Метка комнаты включает версию протокола: старые и новые сборки друг друга не видят.
const LOBBY_TAG: &str = "alembic_tycoon_v4";
const TIMEOUT: f32 = 12.0;

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
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Set(i32, i32, Kind, Dir),
    Clear(i32, i32),
    Dir(i32, i32, Dir),
    Level(i32, i32, u8),
    Ports(i32, i32, u8),
    FilterReset(i32, i32),
}

pub struct StateMsg {
    pub money: f64,
    pub level: u32,
    pub delivered: u32,
    pub upgrades: [u8; 8],
    pub income: f32,
    pub discovered: Vec<bool>,
}

pub enum NetEvent {
    /// Хосту: игрок подключился, ему нужно отправить мир.
    Hello(SteamId, String),
    /// Хосту: команда от игрока.
    Cmd(Cmd),
    /// Клиенту: снимок мира, изменение постройки, состояние экономики.
    World(String),
    Op(Op),
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
    pub id: SteamId,
    pub name: String,
    pub p: Presence,
    silence: f32,
}

pub struct LobbyInfo {
    pub id: LobbyId,
    pub name: String,
    pub members: usize,
}

enum Ev {
    Created(Result<LobbyId, String>),
    Joined(Result<LobbyId, ()>),
    List(Vec<LobbyId>),
    JoinRequested(LobbyId),
    Session(SessionRequest),
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
        _ => return None,
    })
}

// ---------- сеть ----------

pub struct Net {
    client: Option<Client>,
    rx: Receiver<Ev>,
    tx: Sender<Ev>,
    _handles: Vec<CallbackHandle>,
    pub mode: Mode,
    pub status: String,
    pub my_name: String,
    my_id: u64,
    lobby: Option<LobbyId>,
    host: Option<SteamId>,
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
}

impl Net {
    pub fn new() -> Net {
        let (tx, rx) = channel();
        Net {
            client: None,
            rx,
            tx,
            _handles: Vec::new(),
            mode: Mode::Offline,
            status: "НЕ В СЕТИ".to_string(),
            my_name: String::new(),
            my_id: 0,
            lobby: None,
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
        }
    }

    /// Подключается к запущенному клиенту Steam (один раз). false - Steam недоступен.
    pub fn ensure(&mut self) -> bool {
        if self.client.is_some() {
            return true;
        }
        match Client::init_app(APP_ID) {
            Ok(client) => {
                self.my_name = client.friends().name();
                self.my_id = client.user().steam_id().raw();
                let tx = self.tx.clone();
                client.networking_messages().session_request_callback(move |req| {
                    let _ = tx.send(Ev::Session(req));
                });
                let tx = self.tx.clone();
                self._handles.push(client.register_callback(move |r: GameLobbyJoinRequested| {
                    let _ = tx.send(Ev::JoinRequested(r.lobby_steam_id));
                }));
                self.status = format!("STEAM: {}", self.my_name);
                self.client = Some(client);
                true
            }
            Err(e) => {
                self.status = format!("STEAM НЕДОСТУПЕН: {:?}", e).to_uppercase();
                false
            }
        }
    }

    pub fn online(&self) -> bool {
        self.mode != Mode::Offline
    }

    pub fn host_game(&mut self) {
        if !self.ensure() || self.online() || self.busy {
            return;
        }
        let tx = self.tx.clone();
        self.busy = true;
        self.status = "СОЗДАЮ КОМНАТУ...".to_string();
        self.client.as_ref().unwrap().matchmaking().create_lobby(LobbyType::Public, 8, move |r| {
            let _ = tx.send(Ev::Created(r.map_err(|e| format!("{:?}", e))));
        });
    }

    pub fn refresh(&mut self) {
        if !self.ensure() || self.busy {
            return;
        }
        let tx = self.tx.clone();
        self.busy = true;
        self.status = "ИЩУ КОМНАТЫ...".to_string();
        let mm = self.client.as_ref().unwrap().matchmaking();
        mm.add_request_lobby_list_string_filter(StringFilter(LobbyKey::new("game"), LOBBY_TAG, StringFilterKind::Equal));
        mm.set_request_lobby_list_distance_filter(DistanceFilter::Worldwide);
        mm.request_lobby_list(move |r| {
            let _ = tx.send(Ev::List(r.unwrap_or_default()));
        });
    }

    pub fn join(&mut self, lobby: LobbyId) {
        if !self.ensure() || self.busy {
            return;
        }
        if self.online() {
            self.leave();
        }
        let tx = self.tx.clone();
        self.busy = true;
        self.status = "ПОДКЛЮЧАЮСЬ...".to_string();
        self.client.as_ref().unwrap().matchmaking().join_lobby(lobby, move |r| {
            let _ = tx.send(Ev::Joined(r));
        });
    }

    pub fn leave(&mut self) {
        if let Some(client) = self.client.clone() {
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
            if let Some(l) = self.lobby.take() {
                client.matchmaking().leave_lobby(l);
            }
        }
        self.mode = Mode::Offline;
        self.host = None;
        self.peers.clear();
        self.synced = false;
        self.status = "НЕ В СЕТИ".to_string();
    }

    fn send(&self, to: SteamId, data: &[u8], reliable: bool) {
        if let Some(c) = &self.client {
            let flags = if reliable { SendFlags::RELIABLE } else { SendFlags::UNRELIABLE }
                | SendFlags::AUTO_RESTART_BROKEN_SESSION;
            let _ = c.networking_messages().send_message_to_user(NetworkingIdentity::new_steam_id(to), flags, data, 0);
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

    pub fn send_world(&self, to: SteamId, text: &str) {
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
        let mut w = Wr::new(T_STATE).u64(s.money.to_bits()).u32(s.level).u32(s.delivered);
        for u in s.upgrades {
            w = w.u8(u);
        }
        w = w.u32(s.income.to_bits());
        let mut bits = vec![0u8; (N_KINDS + 7) / 8];
        for (i, d) in s.discovered.iter().enumerate().take(N_KINDS) {
            if *d {
                bits[i / 8] |= 1 << (i % 8);
            }
        }
        w.0.extend_from_slice(&bits);
        self.broadcast(&w.0, true);
    }

    /// Подставной игрок для скриншота-самопроверки (в сеть ничего не уходит).
    pub fn fake_peer(&mut self, id: u64, name: &str, p: Presence) {
        self.peers.push(Peer { id: SteamId::from_raw(id), name: name.to_string(), p, silence: 0.0 });
    }

    pub fn set_presence(&mut self, p: Presence) {
        self.cursor = p;
    }

    fn handle(&mut self, from: SteamId, data: &[u8], out: &mut Vec<NetEvent>) {
        let mut r = Rd(data);
        let tag = match r.u8() {
            Some(t) => t,
            None => return,
        };
        match self.mode {
            Mode::Host => {
                if tag == T_HELLO {
                    let name = r.str().unwrap_or_default();
                    if let Some(p) = self.peers.iter_mut().find(|p| p.id == from) {
                        p.silence = 0.0;
                    } else if self.peers.len() < 7 {
                        self.peers.push(Peer { id: from, name: name.clone(), p: Presence::default(), silence: 0.0 });
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
                        let st = (|| {
                            let money = f64::from_bits(r.u64()?);
                            let (level, delivered) = (r.u32()?, r.u32()?);
                            let upgrades = [r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?];
                            let income = f32::from_bits(r.u32()?);
                            let bits = r.take((N_KINDS + 7) / 8)?;
                            let discovered = (0..N_KINDS).map(|i| bits[i / 8] >> (i % 8) & 1 == 1).collect();
                            Some(StateMsg { money, level, delivered, upgrades, income, discovered })
                        })();
                        if let Some(st) = st {
                            out.push(NetEvent::State(st));
                        }
                    }
                    T_CURSORS => {
                        self.peers.clear();
                        while let (Some(id), Some(p), Some(name)) = (r.u64(), rd_pres(&mut r), r.str()) {
                            if id != self.my_id && self.peers.len() < 8 {
                                self.peers.push(Peer { id: SteamId::from_raw(id), name, p, silence: 0.0 });
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

    /// Вызывается каждый кадр: колбэки Steam, входящие сообщения, таймеры.
    pub fn poll(&mut self, dt: f32) -> Vec<NetEvent> {
        let mut out = Vec::new();
        let client = match self.client.clone() {
            Some(c) => c,
            None => return out,
        };
        client.run_callbacks();
        let mm = client.matchmaking();
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Ev::Created(Ok(l)) => {
                    self.busy = false;
                    mm.set_lobby_data(l, "game", LOBBY_TAG);
                    mm.set_lobby_data(l, "name", &self.my_name);
                    self.lobby = Some(l);
                    self.mode = Mode::Host;
                    self.peers.clear();
                    self.status = "ТЫ ХОСТ. КОМНАТА ОТКРЫТА".to_string();
                }
                Ev::Created(Err(e)) => {
                    self.busy = false;
                    self.status = format!("НЕ УДАЛОСЬ СОЗДАТЬ: {}", e).to_uppercase();
                }
                Ev::Joined(Ok(l)) => {
                    self.busy = false;
                    self.lobby = Some(l);
                    self.host = Some(mm.lobby_owner(l));
                    self.mode = Mode::Client;
                    self.synced = false;
                    self.hello_t = 0.0;
                    self.host_silence = 0.0;
                    self.peers.clear();
                    self.status = "ЖДУ МИР ОТ ХОСТА...".to_string();
                }
                Ev::Joined(Err(())) => {
                    self.busy = false;
                    self.status = "НЕ УДАЛОСЬ ВОЙТИ В КОМНАТУ".to_string();
                }
                Ev::List(ids) => {
                    self.busy = false;
                    self.lobbies = ids
                        .into_iter()
                        .take(8)
                        .map(|id| LobbyInfo {
                            id,
                            name: mm.lobby_data(id, "name").unwrap_or_else(|| "?".to_string()),
                            members: mm.lobby_member_count(id),
                        })
                        .collect();
                    self.status = format!("НАЙДЕНО КОМНАТ: {}", self.lobbies.len());
                }
                Ev::JoinRequested(l) => self.join(l),
                Ev::Session(req) => {
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

        let nm = client.networking_messages();
        for _ in 0..8 {
            let msgs = nm.receive_messages_on_channel(0, 64);
            if msgs.is_empty() {
                break;
            }
            for m in msgs {
                if let Some(from) = m.identity_peer().steam_id() {
                    let data = m.data().to_vec();
                    self.handle(from, &data, &mut out);
                }
            }
        }

        self.cursor_t -= dt;
        self.state_t -= dt;
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
                        w = wr_pres(w.u64(p.id.raw()), &p.p).str(&p.name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    #[test]
    fn codec_roundtrip() {
        let cmds = [
            Cmd::Place(Kind(17), 79, 3, Dir::W),
            Cmd::Remove(12, 40),
            Cmd::BeltDir(5, 6, Dir::S),
            Cmd::Upgrade(3),
            Cmd::LevelUp(44, 36),
            Cmd::SetPorts(3, 4, 0b10_01_11_00),
            Cmd::FilterReset(7, 8),
        ];
        for c in cmds {
            let bytes = enc_cmd(&c);
            let back = dec_cmd(&mut Rd(&bytes[1..])).expect("decodes");
            assert_eq!(format!("{:?}", c), format!("{:?}", back));
        }
        let ops = [Op::Set(1, 2, Kind(31), Dir::E), Op::Clear(70, 71), Op::Dir(9, 9, Dir::N), Op::Level(8, 7, 4), Op::Ports(1, 1, 200), Op::FilterReset(2, 2)];
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
