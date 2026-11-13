//! Симуляция фабрики: сетка, конвейеры, машины, экономика и контракты.
//! Постройки и формы предметов описаны таблицами - новую машину можно добавить одной строкой.

use crate::engine::Rng;

pub const W: i32 = 80;
pub const H: i32 = 80;
pub const HUB_X: i32 = 39;
pub const HUB_Y: i32 = 39;
pub const N_UPGRADES: usize = 8;
/// Предел уровня каждого улучшения.
pub const UPGRADE_MAX: [u8; N_UPGRADES] = [15, 15, 15, 15, 10, 10, 10, 10];
/// Предел уровня отдельной машины.
pub const MAX_LEVEL: u8 = 5;

pub const ERR_MONEY: &str = "НЕ ХВАТАЕТ ДЕНЕГ";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub fn from_u8(v: u8) -> Dir {
        match v & 3 {
            0 => Dir::N,
            1 => Dir::E,
            2 => Dir::S,
            _ => Dir::W,
        }
    }
    pub fn dx(self) -> i32 {
        match self {
            Dir::E => 1,
            Dir::W => -1,
            _ => 0,
        }
    }
    pub fn dy(self) -> i32 {
        match self {
            Dir::S => 1,
            Dir::N => -1,
            _ => 0,
        }
    }
    pub fn cw(self) -> Dir {
        Dir::from_u8(self as u8 + 1)
    }
    pub fn ccw(self) -> Dir {
        Dir::from_u8(self as u8 + 3)
    }
    pub fn opp(self) -> Dir {
        Dir::from_u8(self as u8 + 2)
    }
    pub fn from_delta(dx: i32, dy: i32) -> Dir {
        if dx > 0 {
            Dir::E
        } else if dx < 0 {
            Dir::W
        } else if dy > 0 {
            Dir::S
        } else {
            Dir::N
        }
    }
}

// ====================== предметы ======================

/// Предмет: набор стихий (битовая маска), чистота 1..3 и форма (стадия обработки).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Item {
    pub mask: u8,
    pub purity: u8,
    pub form: u8,
}

pub const N_FORMS: usize = 26;
pub const N_KINDS: usize = 15 * N_FORMS;
pub const ELEM_NAMES: [&str; 4] = ["ОГОНЬ", "ВОДА", "ЗЕМЛЯ", "ЭФИР"];
const MIX_NAMES: [&str; 16] = [
    "?", "ОГОНЬ", "ВОДА", "ПАР", "ЗЕМЛЯ", "ЛАВА", "ГЛИНА", "ОБСИДИАН", "ЭФИР", "ПЛАЗМА", "ТУМАН", "ГРОЗА", "ПЫЛЬ",
    "ПЕПЕЛ", "ЖИЗНЬ", "КВИНТЭССЕНЦИЯ",
];
const MIX_GEN: [&str; 16] = [
    "?", "ОГНЯ", "ВОДЫ", "ПАРА", "ЗЕМЛИ", "ЛАВЫ", "ГЛИНЫ", "ОБСИДИАНА", "ЭФИРА", "ПЛАЗМЫ", "ТУМАНА", "ГРОЗЫ", "ПЫЛИ",
    "ПЕПЛА", "ЖИЗНИ", "КВИНТЭССЕНЦИИ",
];
/// Формы: 0 эссенция (сырьё), остальные получаются в машинах.
pub const FORM_NAMES: [&str; N_FORMS] = [
    "ЭССЕНЦИЯ", "КРИСТАЛЛ", "ЗЕЛЬЕ", "ПОРОШОК", "СЛИТОК", "САМОЦВЕТ", "ЭЛИКСИР", "АМУЛЕТ", "РЕЛИКВИЯ", "ДЫМКА", "ЛЁД",
    "ПИЛЮЛЯ", "НИТЬ", "МОНЕТА", "РУНА", "ЛИНЗА", "МАНТИЯ", "КЛИНОК", "ШЕСТЕРНЯ", "ПРИЗМА", "СНАДОБЬЕ", "ГРИМУАР",
    "ГОЛЕМ", "АСТРОЛЯБИЯ", "КОРОНА", "МАГИСТЕРИЙ",
];
/// Множитель цены формы (всегда больше суммы множителей сырья).
const FORM_VALUE: [f64; N_FORMS] = [
    1.0, 2.5, 2.0, 4.0, 6.0, 6.0, 12.0, 25.0, 80.0, 2.0, 2.0, 6.0, 9.0, 9.0, 10.0, 6.0, 22.0, 32.0, 36.0, 16.0, 16.0,
    70.0, 150.0, 110.0, 75.0, 400.0,
];

impl Item {
    pub fn new(mask: u8, purity: u8, form: u8) -> Item {
        Item { mask, purity, form }
    }
    pub fn base(elem: u8) -> Item {
        Item { mask: 1 << elem, purity: 1, form: 0 }
    }
    /// Индекс вида предмета без учёта чистоты (для каталога).
    pub fn kind_idx(self) -> usize {
        (self.mask as usize - 1) + 15 * self.form as usize
    }
    /// Индекс спрайта в Assets::items.
    pub fn id(self) -> usize {
        (self.mask as usize - 1) + 15 * (self.purity as usize - 1) + 45 * self.form as usize
    }
    pub fn value(self) -> f64 {
        let n = [2.0, 8.0, 30.0, 120.0][(self.mask.count_ones() as usize).clamp(1, 4) - 1];
        let p = [1.0, 3.0, 10.0][(self.purity as usize).clamp(1, 3) - 1];
        n * p * FORM_VALUE[(self.form as usize).min(N_FORMS - 1)]
    }
    pub fn name(self) -> String {
        let m = (self.mask & 15) as usize;
        let mut s = if self.form == 0 {
            MIX_NAMES[m].to_string()
        } else {
            format!("{} {}", FORM_NAMES[(self.form as usize).min(N_FORMS - 1)], MIX_GEN[m])
        };
        s.push_str(["", " II", " III"][(self.purity as usize).clamp(1, 3) - 1]);
        s
    }
}

/// Как получить форму: (название машины, из чего).
pub fn form_recipe(form: u8) -> (String, String) {
    if form == 0 {
        return ("ЭКСТРАКТОР, СМЕСИТЕЛЬ".to_string(), "ЖИЛЫ СТИХИЙ".to_string());
    }
    for d in KINDS.iter() {
        match d.role {
            Role::Convert(a, to, _) if to == form => return (d.name.to_string(), FORM_NAMES[a as usize].to_string()),
            Role::Assemble(a, b, to, _) if to == form => {
                return (d.name.to_string(), format!("{} + {}", FORM_NAMES[a as usize], FORM_NAMES[b as usize]))
            }
            _ => {}
        }
    }
    ("?".to_string(), "?".to_string())
}

/// Шаг производственной цепочки: из чего, в какой машине, что получается.
pub struct Step {
    pub inputs: Vec<Item>,
    pub machine: &'static str,
    pub out: Item,
}

/// Полный путь от жил до предмета: шаги по порядку, без повторов.
pub fn recipe_steps(target: Item) -> Vec<Step> {
    fn go(it: Item, out: &mut Vec<Step>) {
        if out.iter().any(|s| s.out == it) {
            return;
        }
        if it.form == 0 {
            if it.purity > 1 {
                let lower = Item::new(it.mask, it.purity - 1, 0);
                go(lower, out);
                out.push(Step { inputs: vec![lower, lower], machine: "ДИСТИЛЛЯТОР", out: it });
            } else if it.mask.count_ones() > 1 {
                let a = Item::new(it.mask & it.mask.wrapping_neg(), 1, 0);
                let b = Item::new(it.mask & !a.mask, 1, 0);
                go(b, out);
                out.push(Step { inputs: vec![a, b], machine: "СМЕСИТЕЛЬ", out: it });
            }
            return;
        }
        for d in KINDS.iter() {
            match d.role {
                Role::Convert(a, to, _) if to == it.form => {
                    let i = Item::new(it.mask, it.purity, a);
                    go(i, out);
                    out.push(Step { inputs: vec![i], machine: d.name, out: it });
                    return;
                }
                Role::Assemble(a, b, to, _) if to == it.form => {
                    let (ia, ib) = (Item::new(it.mask, it.purity, a), Item::new(it.mask, it.purity, b));
                    go(ia, out);
                    go(ib, out);
                    out.push(Step { inputs: vec![ia, ib], machine: d.name, out: it });
                    return;
                }
                _ => {}
            }
        }
    }
    let mut v = Vec::new();
    go(target, &mut v);
    if v.is_empty() {
        v.push(Step { inputs: Vec::new(), machine: "ЭКСТРАКТОР", out: target });
    }
    v
}

// ====================== постройки ======================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    /// Конвейер, множитель скорости.
    Belt(f32),
    /// Добыча из жилы, множитель скорости.
    Extractor(f32),
    Mixer,
    Distiller,
    Splitter,
    Overflow,
    Filter,
    Bridge,
    /// Подземный конвейер: вход и выход смотрят в одну сторону, между ними можно строить.
    Tunnel,
    Storage,
    Trash,
    Trade,
    /// Один вход: форма -> форма, секунды.
    Convert(u8, u8, f32),
    /// Два входа одной смеси: форма + форма -> форма, секунды.
    Assemble(u8, u8, u8, f32),
    Hub,
}

pub struct KindDef {
    pub name: &'static str,
    pub info: &'static str,
    pub cost: f64,
    /// Сколько контрактов нужно выполнить, чтобы открыть постройку.
    pub unlock: usize,
    pub role: Role,
    pub tab: u8,
    pub color: u32,
}

const fn kd(name: &'static str, info: &'static str, cost: f64, unlock: usize, role: Role, tab: u8, color: u32) -> KindDef {
    KindDef { name, info, cost, unlock, role, tab, color }
}

pub const TAB_NAMES: [&str; 5] = ["ЛОГИСТИКА", "ЭССЕНЦИИ", "ОБРАБОТКА", "СБОРКА", "ШЕДЕВРЫ"];

/// Все постройки. Индекс - постоянный идентификатор (он же пишется в сохранение), последняя - лавка.
pub const KINDS: [KindDef; 40] = [
    kd("КОНВЕЙЕР", "ВЕЗЁТ ПРЕДМЕТЫ. ТЯНИ МЫШЬЮ, ЧТОБЫ ПРОЛОЖИТЬ ЛИНИЮ.", 1.0, 0, Role::Belt(1.0), 0, 0x4A4E5A),
    kd("ЭКСТРАКТОР", "СТАВИТСЯ НА ЖИЛУ И КАЧАЕТ ЭССЕНЦИЮ СТИХИИ.", 20.0, 0, Role::Extractor(1.0), 1, 0x8A93A6),
    kd("СМЕСИТЕЛЬ", "СМЕШИВАЕТ ДВЕ ЭССЕНЦИИ БЕЗ ОБЩИХ СТИХИЙ В ОДНУ.", 60.0, 1, Role::Mixer, 1, 0x3FA7A0),
    kd("ДЕЛИТЕЛЬ", "ДЕЛИТ ПОТОК: ВПЕРЁД, ВПРАВО И ВЛЕВО ПО ОЧЕРЕДИ.", 25.0, 2, Role::Splitter, 0, 0xE0B43C),
    kd("МОСТ", "ДВА ПОТОКА ПЕРЕСЕКАЮТСЯ, НЕ СМЕШИВАЯСЬ.", 15.0, 2, Role::Bridge, 0, 0x9B6B43),
    kd("ДИСТИЛЛЯТОР", "ДВЕ ОДИНАКОВЫЕ ЭССЕНЦИИ > ОДНА БОЛЕЕ ЧИСТАЯ (X3 ЦЕНЫ).", 150.0, 3, Role::Distiller, 1, 0xC7773A),
    kd("КРИСТАЛЛИЗАТОР", "", 300.0, 4, Role::Convert(0, 1, 2.5), 1, 0x4F6D8F),
    kd("РАЗЛИВОЧНАЯ", "", 400.0, 5, Role::Convert(0, 2, 1.5), 1, 0xC0508A),
    kd("МЕЛЬНИЦА", "", 800.0, 6, Role::Convert(1, 3, 2.0), 2, 0x8C7B6B),
    kd("ГРАНИЛЬНЯ", "", 1500.0, 7, Role::Convert(1, 5, 3.0), 2, 0x6B4FA0),
    kd("ГОРН", "", 2500.0, 8, Role::Convert(3, 4, 3.0), 2, 0x9E3B2E),
    kd("ИСПАРИТЕЛЬ", "", 2500.0, 8, Role::Convert(0, 9, 1.5), 1, 0x9AA7B8),
    kd("МОРОЗИЛЬНИК", "", 3000.0, 9, Role::Convert(0, 10, 2.0), 1, 0x5FB6D9),
    kd("ПРЕСС", "", 10000.0, 11, Role::Convert(3, 11, 2.0), 2, 0x5E7A8C),
    kd("ВОЛОЧИЛЬНЯ", "", 12000.0, 11, Role::Convert(4, 12, 2.5), 2, 0xB5894A),
    kd("МОНЕТНЫЙ ДВОР", "", 15000.0, 12, Role::Convert(4, 13, 2.5), 2, 0xD9A82E),
    kd("ГРАВЁР", "", 18000.0, 12, Role::Convert(5, 14, 3.0), 2, 0x6E6A7A),
    kd("СТЕКЛОДУВ", "", 22000.0, 13, Role::Convert(3, 15, 2.5), 2, 0x4FB0A0),
    kd("АЛХИМ. СТОЛ", "", 5000.0, 9, Role::Assemble(2, 3, 6, 3.0), 3, 0x6E8B3D),
    kd("ЮВЕЛИР", "", 8000.0, 10, Role::Assemble(5, 4, 7, 3.0), 3, 0xB8478F),
    kd("ТКАЧ", "", 30000.0, 13, Role::Assemble(12, 9, 16, 3.0), 3, 0x7A5CC0),
    kd("ОРУЖЕЙНИК", "", 40000.0, 14, Role::Assemble(4, 14, 17, 3.0), 3, 0x7D8590),
    kd("МЕХАНИК", "", 50000.0, 14, Role::Assemble(12, 13, 18, 3.0), 3, 0xA9772F),
    kd("ОПТИК", "", 60000.0, 15, Role::Assemble(15, 10, 19, 3.0), 3, 0x3D8FBF),
    kd("АПТЕКАРЬ", "", 60000.0, 15, Role::Assemble(11, 2, 20, 3.0), 3, 0x4FA36A),
    kd("РЕЛИКВАРИЙ", "", 100000.0, 16, Role::Assemble(6, 7, 8, 4.0), 4, 0xA8873A),
    kd("ЗЛАТОКУЗНЕЦ", "", 120000.0, 16, Role::Assemble(7, 13, 24, 4.0), 4, 0xC79A2A),
    kd("СКРИПТОРИЙ", "", 200000.0, 17, Role::Assemble(14, 16, 21, 4.0), 4, 0x7A4A3A),
    kd("ГОЛЕМНАЯ", "", 300000.0, 17, Role::Assemble(18, 17, 22, 4.0), 4, 0x5C6470),
    kd("ОБСЕРВАТОРИЯ", "", 400000.0, 18, Role::Assemble(19, 18, 23, 4.0), 4, 0x34407A),
    kd("ВЕЛИКОЕ ДЕЛАНИЕ", "", 1000000.0, 18, Role::Assemble(8, 21, 25, 5.0), 4, 0x8A2A4A),
    kd("ЭКСПРЕСС-ЛЕНТА", "КОНВЕЙЕР С ДВОЙНОЙ СКОРОСТЬЮ.", 5.0, 10, Role::Belt(2.0), 0, 0x3A6FB5),
    kd("ГЛУБИННЫЙ БУР", "КАЧАЕТ ЭССЕНЦИЮ ИЗ ЖИЛЫ В 2.5 РАЗА БЫСТРЕЕ.", 400.0, 11, Role::Extractor(2.5), 1, 0x5A5266),
    kd("ПЕРЕЛИВ", "ПУСКАЕТ ВПЕРЁД, А ЕСЛИ ТАМ ЗАТОР - ВПРАВО ИЛИ ВЛЕВО.", 40.0, 5, Role::Overflow, 0, 0x3D8FBF),
    kd("ФИЛЬТР", "ПЕРВЫЙ ПРЕДМЕТ - ОБРАЗЕЦ: ТАКИЕ ИДУТ ПРЯМО, ОСТАЛЬНЫЕ ВБОК.", 80.0, 7, Role::Filter, 0, 0x8A5CC0),
    kd("СКЛАД", "КОПИТ ДО 100 ОДИНАКОВЫХ ПРЕДМЕТОВ И ВЫДАЁТ ВПЕРЁД.", 100.0, 6, Role::Storage, 0, 0x9B6B43),
    kd("УТИЛИЗАТОР", "УНИЧТОЖАЕТ ВСЁ, ЧТО В НЕГО ПРИВЕЗУТ.", 10.0, 0, Role::Trash, 0, 0x3A3346),
    kd("ТОРГОВЫЙ ПОСТ", "ПРОДАЁТ НА МЕСТЕ ЗА ПОЛЦЕНЫ. В КОНТРАКТЫ НЕ ИДЁТ.", 500.0, 12, Role::Trade, 0, 0xB8873A),
    kd("ПОДЗЕМНЫЙ КОНВЕЙЕР", "НЫРЯЕТ ПОД ПОСТРОЙКИ. ВХОД И ВЫХОД - В ОДНУ СТОРОНУ, НЕ ДАЛЬШЕ 6 КЛЕТОК.", 30.0, 4, Role::Tunnel, 0, 0x6A5A4A),
    kd("ЛАВКА АЛХИМИКА", "ПОКУПАЕТ ВСЁ, ЧТО ПРИВЕЗУТ. СЮДА СДАЮТСЯ КОНТРАКТЫ.", 0.0, 0, Role::Hub, 9, 0x5A5266),
];

pub const N_BUILDABLE: usize = KINDS.len() - 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Kind(pub u8);

#[allow(non_upper_case_globals)]
impl Kind {
    pub const Belt: Kind = Kind(0);
    pub const Extractor: Kind = Kind(1);
    pub const Hub: Kind = Kind(N_BUILDABLE as u8);

    pub fn from_u8(v: u8) -> Option<Kind> {
        if (v as usize) < N_BUILDABLE {
            Some(Kind(v))
        } else {
            None
        }
    }
    pub fn by_name(name: &str) -> Kind {
        Kind(KINDS.iter().position(|d| d.name == name).expect("unknown building") as u8)
    }
    pub fn def(self) -> &'static KindDef {
        &KINDS[self.0 as usize]
    }
    pub fn role(self) -> Role {
        self.def().role
    }
    pub fn cost(self) -> f64 {
        self.def().cost
    }
    pub fn name(self) -> &'static str {
        self.def().name
    }
    pub fn unlock_level(self) -> usize {
        self.def().unlock
    }
    pub fn is_belt(self) -> bool {
        matches!(self.role(), Role::Belt(_))
    }
    /// Лента или мост - то, по чему предметы едут на виду.
    pub fn is_track(self) -> bool {
        matches!(self.role(), Role::Belt(_) | Role::Bridge | Role::Tunnel)
    }
    pub fn desc(self) -> String {
        match self.role() {
            Role::Convert(a, to, _) => format!(
                "{} > {}. ЦЕНА X{}.",
                FORM_NAMES[a as usize], FORM_NAMES[to as usize], FORM_VALUE[to as usize]
            ),
            Role::Assemble(a, b, to, _) => format!(
                "{} + {} ОДНОЙ СМЕСИ > {}. ЦЕНА X{}.",
                FORM_NAMES[a as usize], FORM_NAMES[b as usize], FORM_NAMES[to as usize], FORM_VALUE[to as usize]
            ),
            _ => self.def().info.to_string(),
        }
    }
    /// Постройки вкладки, по порядку открытия.
    pub fn in_tab(tab: usize) -> Vec<Kind> {
        let mut v: Vec<Kind> = (0..N_BUILDABLE as u8).map(Kind).filter(|k| k.def().tab as usize == tab).collect();
        v.sort_by_key(|k| (k.unlock_level(), k.0));
        v
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BeltItem {
    pub item: Item,
    /// Прогресс по тайлу 0..1.
    pub p: f32,
    /// Направление движения при входе на тайл.
    pub d: Dir,
    tick: u32,
}

#[derive(Clone, Debug)]
pub struct Building {
    pub kind: Kind,
    pub dir: Dir,
    /// Полосы конвейера (у моста их две: вертикальная и горизонтальная).
    pub lanes: [Vec<BeltItem>; 2],
    /// Входные ячейки машин; у фильтра slots[1] - образец, у склада slots[0] - что хранится.
    pub slots: [Option<Item>; 2],
    pub out: Option<Item>,
    pub progress: f32,
    pub working: bool,
    /// Количество предметов на складе.
    pub count: u32,
    /// Уровень машины 1..5: каждый следующий даёт +50% скорости.
    pub level: u8,
    /// Режим каждой стороны относительно направления постройки:
    /// [перёд, право, зад, лево], значения PORT_*.
    pub ports: [u8; 4],
    rr: u8,
}

impl Building {
    fn new(kind: Kind, dir: Dir) -> Building {
        Building {
            kind,
            dir,
            lanes: [Vec::new(), Vec::new()],
            slots: [None, None],
            out: None,
            progress: 0.0,
            working: false,
            count: 0,
            level: 1,
            ports: [PORT_AUTO; 4],
            rr: 0,
        }
    }
}

// ====================== стороны построек ======================

/// На сколько клеток вперёд подземный конвейер ищет свой выход.
pub const TUNNEL_RANGE: i32 = 6;
/// Под землёй предметы едут со скоростью экспресс-ленты.
const TUNNEL_SPEED: f32 = 2.0;

pub const PORT_AUTO: u8 = 0;
pub const PORT_IN: u8 = 1;
pub const PORT_OUT: u8 = 2;
pub const PORT_OFF: u8 = 3;

/// Номер стороны относительно направления постройки: 0 перёд, 1 право, 2 зад, 3 лево.
pub fn rel_side(facing: Dir, abs: Dir) -> usize {
    ((abs as u8 + 4 - facing as u8) % 4) as usize
}

fn splitter_like(role: Role) -> bool {
    matches!(role, Role::Splitter | Role::Overflow | Role::Filter)
}

/// Сколько режимов у сторон постройки: 0 - не настраивается, 3 - добыча (без входа), 4 - все.
pub fn port_modes(kind: Kind) -> u8 {
    match kind.role() {
        Role::Extractor(_) => 3,
        Role::Mixer | Role::Distiller | Role::Convert(..) | Role::Assemble(..) | Role::Storage => 4,
        r if splitter_like(r) => 4,
        _ => 0,
    }
}

/// Выдаёт ли постройка предметы через эту сторону.
/// По умолчанию машина выдаёт вперёд, делитель - вперёд и в бока. Если у машины выход
/// назначен вручную, передняя сторона перестаёт быть выходом сама по себе.
pub fn side_out(b: &Building, rel: usize) -> bool {
    match b.ports[rel] {
        PORT_OUT => true,
        PORT_AUTO => {
            if splitter_like(b.kind.role()) {
                rel != 2
            } else {
                rel == 0 && !b.ports.contains(&PORT_OUT)
            }
        }
        _ => false,
    }
}

/// Принимает ли постройка предметы с этой стороны.
pub fn side_in(b: &Building, rel: usize) -> bool {
    let role = b.kind.role();
    if matches!(role, Role::Extractor(_)) {
        return false;
    }
    match b.ports[rel] {
        PORT_IN => true,
        PORT_AUTO => {
            if splitter_like(role) {
                rel == 2
            } else {
                rel != 0
            }
        }
        _ => false,
    }
}

/// Направления всех выходов постройки по порядку: перёд, право, зад, лево.
fn out_dirs(b: &Building) -> ([Dir; 4], usize) {
    let mut v = [Dir::N; 4];
    let mut n = 0;
    for rel in 0..4 {
        if side_out(b, rel) {
            v[n] = Dir::from_u8(b.dir as u8 + rel as u8);
            n += 1;
        }
    }
    (v, n)
}

pub fn pack_ports(p: [u8; 4]) -> u8 {
    p[0] | p[1] << 2 | p[2] << 4 | p[3] << 6
}

pub fn unpack_ports(v: u8) -> [u8; 4] {
    [v & 3, v >> 2 & 3, v >> 4 & 3, v >> 6 & 3]
}

// ====================== контракты ======================

pub struct Contract {
    pub item: Item,
    pub need: u32,
    pub reward: f64,
}

pub const FINAL_LEVEL: usize = 21;

pub fn contract(level: usize) -> Contract {
    let c = |mask, purity, form, need, reward| Contract { item: Item::new(mask, purity, form), need, reward };
    match level {
        0 => c(1, 1, 0, 20, 150.0),
        1 => c(2, 1, 0, 30, 250.0),
        2 => c(3, 1, 0, 40, 500.0),
        3 => c(4, 2, 0, 40, 1000.0),
        4 => c(8, 1, 1, 50, 2000.0),
        5 => c(5, 1, 2, 50, 3000.0),
        6 => c(6, 1, 3, 50, 5000.0),
        7 => c(9, 1, 5, 50, 8000.0),
        8 => c(12, 1, 4, 50, 12000.0),
        9 => c(3, 1, 6, 50, 20000.0),
        10 => c(1, 1, 7, 40, 30000.0),
        11 => c(2, 1, 12, 50, 40000.0),
        12 => c(10, 1, 14, 50, 60000.0),
        13 => c(8, 1, 16, 40, 90000.0),
        14 => c(4, 1, 18, 40, 130000.0),
        15 => c(11, 1, 19, 40, 200000.0),
        16 => c(5, 1, 24, 40, 350000.0),
        17 => c(13, 1, 21, 40, 600000.0),
        18 => c(7, 1, 22, 30, 1200000.0),
        19 => c(14, 1, 23, 30, 2500000.0),
        20 => c(15, 3, 25, 50, 50000000.0),
        n => {
            // Бесконечные контракты после победы.
            let item = Item::new((n * 7 % 15) as u8 + 1, (n % 3) as u8 + 1, (n * 5 % N_FORMS) as u8);
            let need = 100 + 25 * (n - FINAL_LEVEL) as u32;
            Contract { item, need, reward: item.value() * need as f64 * 3.0 }
        }
    }
}

pub const UPGRADE_NAMES: [&str; N_UPGRADES] =
    ["КОНВЕЙЕРЫ", "ДОБЫЧА", "МАШИНЫ", "ЦЕНЫ", "КОНТРАКТЫ", "СКИДКИ", "СКЛАДЫ", "ПОСТЫ"];
pub const UPGRADE_DESC: [&str; N_UPGRADES] = [
    "+25% К СКОРОСТИ КОНВЕЙЕРОВ ЗА УРОВЕНЬ.",
    "+30% К СКОРОСТИ ДОБЫЧИ ЗА УРОВЕНЬ.",
    "+30% К СКОРОСТИ МАШИН ЗА УРОВЕНЬ.",
    "+15% К ЦЕНЕ ПРОДАЖИ ЗА УРОВЕНЬ.",
    "+25% К НАГРАДЕ ЗА КОНТРАКТЫ ЗА УРОВЕНЬ.",
    "-4% К ЦЕНЕ ПОСТРОЕК ЗА УРОВЕНЬ.",
    "+50 МЕСТ НА КАЖДОМ СКЛАДЕ ЗА УРОВЕНЬ.",
    "+5% К ЦЕНЕ ТОРГОВЫХ ПОСТОВ ЗА УРОВЕНЬ.",
];
const UPGRADE_BASE: [f64; N_UPGRADES] = [80.0, 120.0, 150.0, 200.0, 500.0, 400.0, 300.0, 600.0];

/// Множитель скорости машины по её уровню.
pub fn level_mult(level: u8) -> f32 {
    1.0 + 0.5 * (level.clamp(1, MAX_LEVEL) - 1) as f32
}

// ====================== мир ======================

pub struct World {
    pub tiles: Vec<Option<Building>>,
    /// 0 - пусто, 1..4 - жила стихии.
    pub veins: Vec<u8>,
    pub money: f64,
    pub level: usize,
    pub delivered: u32,
    pub upgrades: [u8; N_UPGRADES],
    pub time: f64,
    pub seed: u64,
    pub pending_sold: f64,
    pub pending_levelups: u32,
    pub total_sold: u64,
    /// Какие виды предметов уже продавались (каталог).
    pub discovered: Vec<bool>,
    /// Новинки каталога с премией, ещё не показанные игроку.
    pub pending_new: Vec<(Item, f64)>,
    /// Копия чужого мира в сетевой игре: продажи не меняют экономику, её присылает хост.
    pub replica: bool,
    tick: u32,
}

fn lane_push(l: &mut Vec<BeltItem>, item: Item, d: Dir, p0: f32, tick: u32, cap: usize) -> bool {
    if l.len() >= cap {
        return false;
    }
    let mut p = p0;
    if let Some(last) = l.last() {
        if last.p < 0.5 {
            return false;
        }
        p = p.min(last.p - 0.5);
    }
    l.push(BeltItem { item, p, d, tick });
    true
}

/// Свободная из двух входных ячеек.
fn free_slot(slots: &[Option<Item>; 2]) -> Option<usize> {
    if slots[0].is_none() {
        Some(0)
    } else if slots[1].is_none() {
        Some(1)
    } else {
        None
    }
}

impl World {
    pub fn new(seed: u64) -> World {
        let n = (W * H) as usize;
        let mut w = World {
            tiles: (0..n).map(|_| None).collect(),
            veins: vec![0; n],
            money: 200.0,
            level: 0,
            delivered: 0,
            upgrades: [0; N_UPGRADES],
            time: 0.0,
            seed,
            pending_sold: 0.0,
            pending_levelups: 0,
            total_sold: 0,
            discovered: vec![false; N_KINDS],
            pending_new: Vec::new(),
            replica: false,
            tick: 0,
        };
        w.gen_veins();
        for y in HUB_Y..HUB_Y + 3 {
            for x in HUB_X..HUB_X + 3 {
                w.tiles[(y * W + x) as usize] = Some(Building::new(Kind::Hub, Dir::N));
            }
        }
        w
    }

    fn gen_veins(&mut self) {
        let mut rng = Rng::new(self.seed);
        let (cx, cy) = (HUB_X + 1, HUB_Y + 1);
        for (e, (dx, dy)) in [(-8, -5), (8, -5), (-8, 6), (8, 6)].iter().enumerate() {
            let x = cx + dx + rng.range(-1, 2);
            let y = cy + dy + rng.range(-1, 2);
            self.blob(&mut rng, x, y, e as u8, 12);
        }
        for _ in 0..48 {
            let x = rng.range(3, W - 3);
            let y = rng.range(3, H - 3);
            let e = rng.range(0, 4) as u8;
            let n = rng.range(6, 18);
            if (x - cx).abs().max((y - cy).abs()) >= 15 {
                self.blob(&mut rng, x, y, e, n);
            }
        }
    }

    fn blob(&mut self, rng: &mut Rng, x: i32, y: i32, e: u8, n: i32) {
        let ok = |x: i32, y: i32| {
            x >= 1 && y >= 1 && x < W - 1 && y < H - 1 && (x - HUB_X - 1).abs().max((y - HUB_Y - 1).abs()) >= 4
        };
        let mut cells = vec![(x, y)];
        let mut guard = 0;
        while (cells.len() as i32) < n && guard < 600 {
            guard += 1;
            let (bx, by) = cells[rng.range(0, cells.len() as i32) as usize];
            let d = Dir::from_u8(rng.range(0, 4) as u8);
            let p = (bx + d.dx(), by + d.dy());
            if ok(p.0, p.1) && !cells.contains(&p) {
                cells.push(p);
            }
        }
        for (x, y) in cells {
            if ok(x, y) {
                self.veins[(y * W + x) as usize] = e + 1;
            }
        }
    }

    pub fn inb(x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < W && y < H
    }

    pub fn get(&self, x: i32, y: i32) -> Option<&Building> {
        if Self::inb(x, y) {
            self.tiles[(y * W + x) as usize].as_ref()
        } else {
            None
        }
    }

    pub fn vein(&self, x: i32, y: i32) -> u8 {
        if Self::inb(x, y) {
            self.veins[(y * W + x) as usize]
        } else {
            0
        }
    }

    // ---------- параметры с учётом улучшений ----------

    pub fn belt_speed(&self) -> f32 {
        2.0 * (1.0 + 0.25 * self.upgrades[0] as f32)
    }
    pub fn extract_rate(&self) -> f32 {
        0.8 * (1.0 + 0.3 * self.upgrades[1] as f32)
    }
    pub fn machine_speed(&self) -> f32 {
        1.0 + 0.3 * self.upgrades[2] as f32
    }
    pub fn price_mult(&self) -> f64 {
        1.0 + 0.15 * self.upgrades[3] as f64
    }
    pub fn reward_mult(&self) -> f64 {
        1.0 + 0.25 * self.upgrades[4] as f64
    }
    pub fn storage_cap(&self) -> u32 {
        100 + 50 * self.upgrades[6] as u32
    }
    pub fn trade_mult(&self) -> f64 {
        0.5 + 0.05 * self.upgrades[7] as f64
    }
    /// Цена постройки с учётом улучшения "Скидки".
    pub fn cost(&self, kind: Kind) -> f64 {
        (kind.cost() * (1.0 - 0.04 * self.upgrades[5] as f64)).ceil().max(1.0)
    }
    /// Сколько вернётся при сносе: цена постройки и всех купленных уровней.
    fn refund(&self, kind: Kind, level: u8) -> f64 {
        self.cost(kind) * (2f64.powi(level.clamp(1, MAX_LEVEL) as i32) - 1.0)
    }
    pub fn upgrade_cost(&self, i: usize) -> Option<f64> {
        let lvl = self.upgrades[i];
        if lvl >= UPGRADE_MAX[i] {
            return None;
        }
        Some((UPGRADE_BASE[i] * 2.2f64.powi(lvl as i32)).round())
    }
    pub fn buy_upgrade(&mut self, i: usize) -> Result<(), &'static str> {
        match self.upgrade_cost(i) {
            None => Err("МАКСИМАЛЬНЫЙ УРОВЕНЬ"),
            Some(c) if c > self.money => Err(ERR_MONEY),
            Some(c) => {
                self.money -= c;
                self.upgrades[i] += 1;
                Ok(())
            }
        }
    }

    // ---------- строительство ----------

    pub fn can_place(&self, kind: Kind, x: i32, y: i32) -> Result<(), &'static str> {
        if !Self::inb(x, y) {
            return Err("ЗА ГРАНИЦЕЙ КАРТЫ");
        }
        if let Some(b) = self.get(x, y) {
            if b.kind == Kind::Hub {
                return Err("ЗДЕСЬ СТОИТ ЛАВКА");
            }
        }
        if matches!(kind.role(), Role::Extractor(_)) && self.vein(x, y) == 0 {
            return Err("ДОБЫЧА СТАВИТСЯ ТОЛЬКО НА ЖИЛУ");
        }
        Ok(())
    }

    /// Ok(true) - что-то изменилось, Ok(false) - уже стоит то же самое.
    pub fn place(&mut self, kind: Kind, x: i32, y: i32, dir: Dir) -> Result<bool, &'static str> {
        self.can_place(kind, x, y)?;
        let idx = (y * W + x) as usize;
        let mut old = None;
        if let Some(b) = self.tiles[idx].as_mut() {
            if b.kind == kind {
                if b.dir == dir {
                    return Ok(false);
                }
                b.dir = dir;
                return Ok(true);
            }
            old = Some((b.kind, b.level));
        }
        let refund = old.map_or(0.0, |(k, l)| self.refund(k, l));
        let cost = self.cost(kind);
        if self.money + refund < cost {
            return Err(ERR_MONEY);
        }
        self.money += refund - cost;
        self.tiles[idx] = Some(Building::new(kind, dir));
        Ok(true)
    }

    pub fn set_belt_dir(&mut self, x: i32, y: i32, dir: Dir) {
        if !Self::inb(x, y) {
            return;
        }
        if let Some(b) = self.tiles[(y * W + x) as usize].as_mut() {
            if b.kind.is_belt() {
                b.dir = dir;
            }
        }
    }

    /// Сносит постройку и возвращает её стоимость.
    pub fn remove(&mut self, x: i32, y: i32) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        let idx = (y * W + x) as usize;
        match &self.tiles[idx] {
            Some(b) if b.kind != Kind::Hub => {
                self.money += self.refund(b.kind, b.level);
                self.tiles[idx] = None;
                true
            }
            _ => false,
        }
    }

    // ---------- уровни машин ----------

    /// Можно ли вообще прокачивать такую постройку (то, что добывает или перерабатывает).
    pub fn can_level(kind: Kind) -> bool {
        matches!(
            kind.role(),
            Role::Extractor(_) | Role::Mixer | Role::Distiller | Role::Convert(..) | Role::Assemble(..)
        )
    }

    /// Цена следующего уровня машины на тайле; None - улучшать нечего или уже максимум.
    pub fn level_cost(&self, x: i32, y: i32) -> Option<f64> {
        let b = self.get(x, y)?;
        if !Self::can_level(b.kind) || b.level >= MAX_LEVEL {
            return None;
        }
        Some(self.cost(b.kind) * 2f64.powi(b.level as i32))
    }

    pub fn level_up(&mut self, x: i32, y: i32) -> Result<u8, &'static str> {
        let cost = match self.level_cost(x, y) {
            Some(c) => c,
            None => return Err("ЭТО НЕЛЬЗЯ УЛУЧШИТЬ"),
        };
        if cost > self.money {
            return Err(ERR_MONEY);
        }
        self.money -= cost;
        let b = self.tiles[(y * W + x) as usize].as_mut().unwrap();
        b.level += 1;
        Ok(b.level)
    }

    pub fn force_level(&mut self, x: i32, y: i32, level: u8) {
        if Self::inb(x, y) {
            if let Some(b) = self.tiles[(y * W + x) as usize].as_mut() {
                if Self::can_level(b.kind) {
                    b.level = level.clamp(1, MAX_LEVEL);
                }
            }
        }
    }

    // ---------- стороны и фильтры ----------

    /// Какой станет настройка сторон, если переключить сторону `rel` на следующий режим.
    pub fn next_ports(&self, x: i32, y: i32, rel: usize) -> Option<u8> {
        let b = self.get(x, y)?;
        let mut p = b.ports;
        p[rel] = match port_modes(b.kind) {
            0 => return None,
            3 => match p[rel] {
                PORT_AUTO => PORT_OUT,
                PORT_OUT => PORT_OFF,
                _ => PORT_AUTO,
            },
            _ => (p[rel] + 1) % 4,
        };
        Some(pack_ports(p))
    }

    pub fn set_ports(&mut self, x: i32, y: i32, packed: u8) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        match self.tiles[(y * W + x) as usize].as_mut() {
            Some(b) if port_modes(b.kind) > 0 => {
                b.ports = unpack_ports(packed);
                if port_modes(b.kind) == 3 {
                    for p in &mut b.ports {
                        if *p == PORT_IN {
                            *p = PORT_AUTO;
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Сбрасывает образец фильтра: следующий предмет станет новым образцом.
    pub fn filter_reset(&mut self, x: i32, y: i32) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        match self.tiles[(y * W + x) as usize].as_mut() {
            Some(b) if b.kind.role() == Role::Filter => {
                b.slots[1] = None;
                true
            }
            _ => false,
        }
    }

    // ---------- сетевая копия ----------

    /// Ставит постройку без проверок и денег (так клиент повторяет решения хоста).
    pub fn force_set(&mut self, kind: Kind, x: i32, y: i32, dir: Dir) {
        if !Self::inb(x, y) || kind == Kind::Hub {
            return;
        }
        let idx = (y * W + x) as usize;
        match self.tiles[idx].as_mut() {
            Some(b) if b.kind == Kind::Hub => {}
            Some(b) if b.kind == kind => b.dir = dir,
            _ => self.tiles[idx] = Some(Building::new(kind, dir)),
        }
    }

    pub fn force_clear(&mut self, x: i32, y: i32) {
        if Self::inb(x, y) {
            let idx = (y * W + x) as usize;
            if self.tiles[idx].as_ref().map_or(false, |b| b.kind != Kind::Hub) {
                self.tiles[idx] = None;
            }
        }
    }

    /// Принимает экономику от хоста; новые уровни и новинки каталога превращает в события.
    pub fn apply_state(&mut self, money: f64, level: usize, delivered: u32, upgrades: [u8; N_UPGRADES], found: &[bool]) {
        if level > self.level {
            self.pending_levelups += 1;
        }
        for i in 0..N_KINDS.min(found.len()) {
            if found[i] && !self.discovered[i] {
                self.discovered[i] = true;
                let it = Item::new((i % 15) as u8 + 1, 1, (i / 15) as u8);
                self.pending_new.push((it, it.value() * 10.0));
            }
        }
        self.money = money;
        self.level = level;
        self.delivered = delivered;
        for i in 0..N_UPGRADES {
            self.upgrades[i] = upgrades[i].min(UPGRADE_MAX[i]);
        }
    }

    // ---------- симуляция ----------

    pub fn update(&mut self, dt: f32) {
        // Шаг дробится так, чтобы даже экспресс-лента не проскакивала больше 0.2 тайла.
        let n = ((self.belt_speed() * 2.0 * dt / 0.2).ceil() as i32).clamp(1, 24);
        for _ in 0..n {
            self.step(dt / n as f32);
        }
    }

    fn sell(&mut self, item: Item, mult: f64, at_hub: bool) {
        if self.replica {
            return;
        }
        let v = item.value() * self.price_mult() * mult;
        self.money += v;
        self.pending_sold += v;
        self.total_sold += 1;
        if !self.discovered[item.kind_idx()] {
            self.discovered[item.kind_idx()] = true;
            let bonus = Item::new(item.mask, 1, item.form).value() * 10.0;
            self.money += bonus;
            self.pending_new.push((item, bonus));
        }
        if !at_hub {
            return;
        }
        let c = contract(self.level);
        if item == c.item {
            self.delivered += 1;
            if self.delivered >= c.need {
                self.money += c.reward * self.reward_mult();
                self.level += 1;
                self.delivered = 0;
                self.pending_levelups += 1;
            }
        }
    }

    /// Пытается передать предмет, движущийся в направлении `d`, на тайл (x, y).
    fn try_insert(&mut self, x: i32, y: i32, item: Item, d: Dir, p0: f32) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        let idx = (y * W + x) as usize;
        let role = match &self.tiles[idx] {
            Some(b) => b.kind.role(),
            None => return false,
        };
        match role {
            Role::Hub => {
                self.sell(item, 1.0, true);
                return true;
            }
            Role::Trade => {
                let m = self.trade_mult();
                self.sell(item, m, false);
                return true;
            }
            Role::Trash => return true,
            _ => {}
        }
        let tick = self.tick;
        let cap = self.storage_cap();
        // (это выход, длина подземного участка в клетках)
        let tunnel = if role == Role::Tunnel {
            (self.tunnel_is_exit(x, y), self.tunnel_exit(x, y).map_or(1, |e| e.2 as usize))
        } else {
            (false, 1)
        };
        let b = self.tiles[idx].as_mut().unwrap();
        let from_front = d == b.dir.opp();
        // Сторона, с которой предмет входит, и открыта ли она на приём.
        let closed = !side_in(b, rel_side(b.dir, d.opp()));
        match role {
            Role::Belt(_) => !from_front && lane_push(&mut b.lanes[0], item, d, p0, tick, 2),
            // Вход вмещает всё, что едет под землёй до выхода; сам выход с поверхности не принимает.
            Role::Tunnel => !from_front && !tunnel.0 && lane_push(&mut b.lanes[0], item, d, p0, tick, 2 * tunnel.1),
            Role::Bridge => {
                let l = &mut b.lanes[(d as usize) % 2];
                if l.first().map_or(false, |f| f.d != d) {
                    return false;
                }
                lane_push(l, item, d, p0, tick, 2)
            }
            Role::Splitter | Role::Overflow | Role::Filter => {
                if closed || b.slots[0].is_some() {
                    return false;
                }
                b.slots[0] = Some(item);
                true
            }
            Role::Mixer | Role::Distiller | Role::Assemble(..) => {
                if closed {
                    return false;
                }
                let free = match free_slot(&b.slots) {
                    Some(f) => f,
                    None => return false,
                };
                let other = b.slots[1 - free];
                let ok = match role {
                    Role::Mixer => item.form == 0 && other.map_or(true, |o| o.mask & item.mask == 0),
                    Role::Distiller => item.form == 0 && item.purity < 3 && other.map_or(true, |o| o == item),
                    Role::Assemble(fa, fb, _, _) => {
                        (item.form == fa || item.form == fb)
                            && other.map_or(true, |o| o.mask == item.mask && o.form != item.form)
                    }
                    _ => false,
                };
                if ok {
                    b.slots[free] = Some(item);
                }
                ok
            }
            Role::Convert(from, _, _) => {
                if closed || item.form != from || b.slots[0].is_some() {
                    return false;
                }
                b.slots[0] = Some(item);
                true
            }
            Role::Storage => {
                if closed || b.count >= cap || (b.count > 0 && b.slots[0] != Some(item)) {
                    return false;
                }
                b.slots[0] = Some(item);
                b.count += 1;
                true
            }
            Role::Extractor(_) | Role::Hub | Role::Trade | Role::Trash => false,
        }
    }

    // ---------- подземный конвейер ----------

    fn tunnel_dir(&self, x: i32, y: i32) -> Option<Dir> {
        self.get(x, y).filter(|b| b.kind.role() == Role::Tunnel).map(|b| b.dir)
    }

    /// Выход ли это: позади в пределах дальности есть подземный конвейер того же направления,
    /// который сам не является выходом. Так цепочка в одну линию делится на пары вход-выход.
    pub fn tunnel_is_exit(&self, x: i32, y: i32) -> bool {
        let d = match self.tunnel_dir(x, y) {
            Some(d) => d,
            None => return false,
        };
        for k in 1..=TUNNEL_RANGE {
            let (bx, by) = (x - d.dx() * k, y - d.dy() * k);
            if self.tunnel_dir(bx, by) == Some(d) {
                return !self.tunnel_is_exit(bx, by);
            }
        }
        false
    }

    /// Клетка выхода для входа (x, y) и расстояние до неё; None - это выход или пары нет
    /// (одиночный подземный конвейер работает как обычная лента).
    pub fn tunnel_exit(&self, x: i32, y: i32) -> Option<(i32, i32, i32)> {
        let d = self.tunnel_dir(x, y)?;
        if self.tunnel_is_exit(x, y) {
            return None;
        }
        (1..=TUNNEL_RANGE)
            .map(|k| (x + d.dx() * k, y + d.dy() * k, k))
            .find(|&(ex, ey, _)| self.tunnel_dir(ex, ey) == Some(d))
    }

    fn advance_lane(&mut self, i: usize, x: i32, y: i32, lane: usize, out: Option<Dir>, adv: f32) {
        self.advance_lane_to(i, x, y, lane, out, adv, 1.0, None);
    }

    /// Двигает полосу длиной `len` клеток. Если задан `exit` - это подземный участок:
    /// доехавший предмет появляется сразу на клетке выхода.
    #[allow(clippy::too_many_arguments)]
    fn advance_lane_to(
        &mut self,
        i: usize,
        x: i32,
        y: i32,
        lane: usize,
        out: Option<Dir>,
        adv: f32,
        len: f32,
        exit: Option<usize>,
    ) {
        let tick = self.tick;
        let front = match self.tiles[i].as_ref().and_then(|b| b.lanes[lane].first()) {
            Some(f) => *f,
            None => return,
        };
        let mut start = 1;
        if front.tick != tick {
            let od = out.unwrap_or(front.d);
            let np = front.p + adv;
            if np >= len {
                let p0 = (np - len).min(0.45);
                let moved = match exit {
                    Some(e) => lane_push(&mut self.tiles[e].as_mut().unwrap().lanes[0], front.item, od, p0, tick, 2),
                    None => self.try_insert(x + od.dx(), y + od.dy(), front.item, od, p0),
                };
                if moved {
                    self.tiles[i].as_mut().unwrap().lanes[lane].remove(0);
                    start = 0;
                } else {
                    self.tiles[i].as_mut().unwrap().lanes[lane][0].p = len;
                }
            } else {
                self.tiles[i].as_mut().unwrap().lanes[lane][0].p = np;
            }
        }
        let l = &mut self.tiles[i].as_mut().unwrap().lanes[lane];
        for j in start..l.len() {
            if l[j].tick == tick {
                continue;
            }
            let lim = if j == 0 { len } else { l[j - 1].p - 0.5 };
            let np = (l[j].p + adv).min(lim);
            if np > l[j].p {
                l[j].p = np;
            }
        }
    }

    /// Выдаёт готовый предмет через выходы постройки; если их несколько - по очереди.
    fn push_out(&mut self, i: usize, x: i32, y: i32, _dir: Dir) {
        let (it, outs, n, rr) = match self.tiles[i].as_ref() {
            Some(b) => match b.out {
                Some(it) => {
                    let (outs, n) = out_dirs(b);
                    (it, outs, n, b.rr as usize)
                }
                None => return,
            },
            None => return,
        };
        for k in 0..n {
            let od = outs[(rr + k) % n];
            if self.try_insert(x + od.dx(), y + od.dy(), it, od, 0.0) {
                let b = self.tiles[i].as_mut().unwrap();
                b.out = None;
                b.rr = ((rr + k + 1) % 12) as u8;
                break;
            }
        }
    }

    fn step(&mut self, dt: f32) {
        self.time += dt as f64;
        self.tick = self.tick.wrapping_add(1);
        let adv = self.belt_speed() * dt;
        let er = self.extract_rate() * dt;
        let ms = self.machine_speed() * dt;
        for i in 0..self.tiles.len() {
            let (role, dir) = match &self.tiles[i] {
                Some(b) => (b.kind.role(), b.dir),
                None => continue,
            };
            let x = i as i32 % W;
            let y = i as i32 / W;
            match role {
                Role::Hub | Role::Trash | Role::Trade => {}
                Role::Belt(m) => self.advance_lane(i, x, y, 0, Some(dir), adv * m),
                Role::Tunnel => match self.tunnel_exit(x, y) {
                    Some((ex, ey, dist)) => {
                        let e = (ey * W + ex) as usize;
                        self.advance_lane_to(i, x, y, 0, Some(dir), adv * TUNNEL_SPEED, dist as f32, Some(e));
                    }
                    None => self.advance_lane(i, x, y, 0, Some(dir), adv * TUNNEL_SPEED),
                },
                Role::Bridge => {
                    self.advance_lane(i, x, y, 0, None, adv);
                    self.advance_lane(i, x, y, 1, None, adv);
                }
                Role::Extractor(m) => {
                    let v = self.veins[i];
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if v > 0 && b.out.is_none() {
                        b.working = true;
                        b.progress += er * m * level_mult(b.level);
                        if b.progress >= 1.0 {
                            b.progress = 0.0;
                            b.out = Some(Item::base(v - 1));
                        }
                    }
                    self.push_out(i, x, y, dir);
                }
                Role::Mixer | Role::Distiller | Role::Assemble(..) => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if b.out.is_none() {
                        if let (Some(a), Some(c)) = (b.slots[0], b.slots[1]) {
                            let minp = a.purity.min(c.purity);
                            let (t, res) = match role {
                                Role::Mixer => (1.0, Item::new(a.mask | c.mask, minp, 0)),
                                Role::Distiller => (1.5, Item::new(a.mask, a.purity + 1, 0)),
                                Role::Assemble(_, _, to, t) => (t, Item::new(a.mask, minp, to)),
                                _ => (1.0, a),
                            };
                            b.working = true;
                            b.progress += ms * level_mult(b.level) / t;
                            if b.progress >= 1.0 {
                                b.progress = 0.0;
                                b.slots = [None, None];
                                b.out = Some(res);
                            }
                        }
                    }
                    self.push_out(i, x, y, dir);
                }
                Role::Convert(_, to, t) => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if b.out.is_none() {
                        if let Some(a) = b.slots[0] {
                            b.working = true;
                            b.progress += ms * level_mult(b.level) / t;
                            if b.progress >= 1.0 {
                                b.progress = 0.0;
                                b.slots[0] = None;
                                b.out = Some(Item::new(a.mask, a.purity, to));
                            }
                        }
                    }
                    self.push_out(i, x, y, dir);
                }
                Role::Storage => {
                    let b = self.tiles[i].as_mut().unwrap();
                    if b.out.is_none() && b.count > 0 {
                        b.out = b.slots[0];
                        b.count -= 1;
                    }
                    self.push_out(i, x, y, dir);
                }
                Role::Splitter | Role::Overflow | Role::Filter => {
                    let (it, rr, sample, all4, cnt) = match self.tiles[i].as_ref() {
                        Some(b) => match b.slots[0] {
                            Some(it) => {
                                let (o, n) = out_dirs(b);
                                (it, b.rr, b.slots[1], o, n)
                            }
                            None => continue,
                        },
                        None => continue,
                    };
                    if cnt == 0 {
                        continue;
                    }
                    let all = &all4[..cnt];
                    let (outs, rot): (&[Dir], u8) = match role {
                        Role::Splitter => (&all[..], rr),
                        Role::Overflow => (&all[..], 0),
                        _ => {
                            let sample = sample.unwrap_or(it);
                            self.tiles[i].as_mut().unwrap().slots[1] = Some(sample);
                            if sample == it {
                                (&all[..1], 0)
                            } else {
                                (&all[1..], rr % 2)
                            }
                        }
                    };
                    let n = outs.len() as u8;
                    for k in 0..n {
                        let od = outs[((rot + k) % n) as usize];
                        if self.try_insert(x + od.dx(), y + od.dy(), it, od, 0.0) {
                            let b = self.tiles[i].as_mut().unwrap();
                            b.slots[0] = None;
                            b.rr = (rot + k + 1) % 12;
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Выдаёт ли сосед со стороны `side` предметы на тайл (x, y)? Нужно для отрисовки стыков.
    pub fn feeds_from(&self, x: i32, y: i32, side: Dir) -> bool {
        match self.get(x + side.dx(), y + side.dy()) {
            Some(n) => match n.kind.role() {
                Role::Hub | Role::Trash | Role::Trade => false,
                Role::Bridge => true,
                Role::Belt(_) => n.dir == side.opp(),
                // Вход пары уводит предметы под землю, наружу выдаёт только выход или одиночка.
                Role::Tunnel => {
                    n.dir == side.opp() && self.tunnel_exit(x + side.dx(), y + side.dy()).is_none()
                }
                _ => side_out(n, rel_side(n.dir, side.opp())),
            },
            None => false,
        }
    }

    /// Принимает ли тайл (x, y) предметы, едущие в направлении `d`? Тоже для отрисовки стыков.
    pub fn accepts_from(&self, x: i32, y: i32, d: Dir) -> bool {
        match self.get(x, y) {
            Some(n) => match n.kind.role() {
                Role::Hub | Role::Trash | Role::Trade | Role::Bridge => true,
                Role::Belt(_) => n.dir != d.opp(),
                Role::Tunnel => n.dir != d.opp() && !self.tunnel_is_exit(x, y),
                _ => side_in(n, rel_side(n.dir, d.opp())),
            },
            None => false,
        }
    }

    // ---------- сохранение ----------

    pub fn save(&self) -> String {
        let u = self.upgrades;
        let mut s = format!(
            "ALEMBIC1\n{} {} {} {} {} {} {} {}\n",
            self.money.floor() as u64,
            self.level,
            self.delivered,
            u[0],
            u[1],
            u[2],
            u[3],
            self.seed
        );
        s.push_str("D ");
        s.extend(self.discovered.iter().map(|d| if *d { '1' } else { '0' }));
        s.push('\n');
        s.push_str(&format!("U {} {} {} {}\n", u[4], u[5], u[6], u[7]));
        for (i, t) in self.tiles.iter().enumerate() {
            if let Some(b) = t {
                if b.kind != Kind::Hub {
                    s.push_str(&format!("{} {} {} {} {}", i as i32 % W, i as i32 / W, b.kind.0, b.dir as u8, b.level));
                    // Настройка сторон пишется шестым числом, только если она не по умолчанию.
                    if b.ports != [PORT_AUTO; 4] {
                        s.push_str(&format!(" {}", pack_ports(b.ports)));
                    }
                    s.push('\n');
                }
            }
        }
        s
    }

    pub fn load(text: &str) -> Option<World> {
        let mut lines = text.lines();
        if lines.next()? != "ALEMBIC1" {
            return None;
        }
        let head: Vec<u64> = lines.next()?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if head.len() != 8 {
            return None;
        }
        let mut w = World::new(head[7]);
        w.money = head[0] as f64;
        w.level = head[1] as usize;
        w.delivered = head[2] as u32;
        for i in 0..4 {
            w.upgrades[i] = (head[3 + i] as u8).min(UPGRADE_MAX[i]);
        }
        for line in lines {
            if let Some(d) = line.strip_prefix("D ") {
                for (i, ch) in d.chars().take(N_KINDS).enumerate() {
                    w.discovered[i] = ch == '1';
                }
                continue;
            }
            if let Some(u) = line.strip_prefix("U ") {
                // Новые улучшения хранятся отдельной строкой: старые сохранения её просто не содержат.
                for (i, v) in u.split_whitespace().filter_map(|v| v.parse::<u8>().ok()).take(4).enumerate() {
                    w.upgrades[4 + i] = v.min(UPGRADE_MAX[4 + i]);
                }
                continue;
            }
            let v: Vec<i32> = line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            // Пятое число - уровень машины; в старых сохранениях его нет, тогда уровень 1.
            if v.len() < 4 || v.len() > 6 || !Self::inb(v[0], v[1]) {
                continue;
            }
            if let Some(kind) = Kind::from_u8(v[2] as u8) {
                if w.can_place(kind, v[0], v[1]).is_ok() {
                    let mut b = Building::new(kind, Dir::from_u8(v[3] as u8));
                    if Self::can_level(kind) {
                        b.level = (*v.get(4).unwrap_or(&1)).clamp(1, MAX_LEVEL as i32) as u8;
                    }
                    if port_modes(kind) > 0 {
                        b.ports = unpack_ports(*v.get(5).unwrap_or(&0) as u8);
                    }
                    w.tiles[(v[1] * W + v[0]) as usize] = Some(b);
                }
            }
        }
        Some(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new(1);
        w.money = 1e9;
        w.level = 30;
        w
    }

    fn run(w: &mut World, secs: f32) {
        for _ in 0..(secs * 60.0) as u32 {
            w.update(1.0 / 60.0);
        }
    }

    /// Предметы проходят под постройками между входом и выходом и не попадают в них.
    #[test]
    fn tunnel_passes_under_buildings() {
        let mut w = world();
        let tunnel = Kind::by_name("ПОДЗЕМНЫЙ КОНВЕЙЕР");
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 10, Dir::E).unwrap();
        w.place(tunnel, 12, 10, Dir::E).unwrap();
        // Над подземным участком: утилизатор, который съел бы всё, и поперечная лента.
        w.place(Kind::by_name("УТИЛИЗАТОР"), 13, 10, Dir::N).unwrap();
        w.place(Kind::Belt, 14, 10, Dir::S).unwrap();
        w.place(Kind::Belt, 14, 11, Dir::S).unwrap();
        w.place(tunnel, 15, 10, Dir::E).unwrap();
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 16, 10, Dir::N).unwrap();
        assert!(!w.tunnel_is_exit(12, 10) && w.tunnel_is_exit(15, 10));
        assert_eq!(w.tunnel_exit(12, 10), Some((15, 10, 3)));
        run(&mut w, 20.0);
        assert!(w.total_sold >= 10, "продано {}", w.total_sold);
        assert!(w.get(14, 10).unwrap().lanes[0].is_empty() && w.get(14, 11).unwrap().lanes[0].is_empty());
        // Выход не принимает предметы с поверхности, вход - принимает сбоку.
        assert!(!w.try_insert(15, 10, Item::base(0), Dir::S, 0.0));
        assert!(w.try_insert(12, 10, Item::base(0), Dir::S, 0.0));
    }

    /// Без пары и слишком далеко от пары подземный конвейер работает как обычная лента;
    /// четыре в ряд делятся на две пары.
    #[test]
    fn tunnel_pairing() {
        let mut w = world();
        let tunnel = Kind::by_name("ПОДЗЕМНЫЙ КОНВЕЙЕР");
        w.place(tunnel, 5, 5, Dir::S).unwrap();
        w.place(tunnel, 5, 5 + TUNNEL_RANGE + 1, Dir::S).unwrap();
        assert_eq!(w.tunnel_exit(5, 5), None);
        assert!(!w.tunnel_is_exit(5, 5 + TUNNEL_RANGE + 1));
        // Встречный и поперечный не считаются парой.
        w.place(tunnel, 5, 7, Dir::N).unwrap();
        w.place(tunnel, 5, 8, Dir::E).unwrap();
        assert_eq!(w.tunnel_exit(5, 5), None);
        for x in [20, 22, 24, 26] {
            w.place(tunnel, x, 20, Dir::W).unwrap();
        }
        assert_eq!(w.tunnel_exit(26, 20), Some((24, 20, 2)));
        assert_eq!(w.tunnel_exit(24, 20), None);
        assert_eq!(w.tunnel_exit(22, 20), Some((20, 20, 2)));
        // Сохранение переносит подземные конвейеры как любую постройку.
        let back = World::load(&w.save()).unwrap();
        assert_eq!(back.tunnel_exit(26, 20), Some((24, 20, 2)));
    }

    /// Если выход занят, предметы копятся под землёй, а не пропадают.
    #[test]
    fn tunnel_backs_up() {
        let mut w = world();
        let tunnel = Kind::by_name("ПОДЗЕМНЫЙ КОНВЕЙЕР");
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(tunnel, 11, 10, Dir::E).unwrap();
        w.place(tunnel, 15, 10, Dir::E).unwrap();
        run(&mut w, 60.0);
        // Тупик: 2 предмета на выходе и по 2 на каждую из 4 клеток подземного участка.
        assert_eq!(w.get(15, 10).unwrap().lanes[0].len(), 2);
        assert_eq!(w.get(11, 10).unwrap().lanes[0].len(), 8);
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 16, 10, Dir::N).unwrap();
        run(&mut w, 10.0);
        assert!(w.total_sold >= 10);
    }
}
