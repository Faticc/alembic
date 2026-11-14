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
    /// Ящик из CRATE_SIZE таких предметов (делает упаковщик).
    pub boxed: bool,
}

/// Сколько одинаковых предметов упаковщик кладёт в один ящик.
pub const CRATE_SIZE: u32 = 8;

pub const N_FORMS: usize = 26;
/// Все формы, включая те, что не входят в сетку каталога (философский камень).
pub const N_FORMS_ALL: usize = N_FORMS + 1;
/// Число спрайтов предметов: Item::id() всегда меньше.
pub const N_ALL_IDS: usize = 45 * N_FORMS_ALL;
pub const FORM_MAGISTERIUM: u8 = 25;
/// Философский камень: форма-вершина вне сетки каталога, бывает только квинтэссенцией.
pub const FORM_STONE: u8 = 26;
const STONE_NAME: &str = "ФИЛОСОФСКИЙ КАМЕНЬ";
/// Множитель цены камня: больше суммы четырёх магистериев, из которых он собран.
const STONE_VALUE: f64 = 2000.0;
pub const N_KINDS: usize = 15 * N_FORMS;
/// Число разных предметов с учётом чистоты: 15 смесей x 3 чистоты x 26 форм.
pub const N_ITEM_IDS: usize = 45 * N_FORMS;
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

pub fn form_name(form: u8) -> &'static str {
    if form == FORM_STONE {
        return STONE_NAME;
    }
    FORM_NAMES[(form as usize).min(N_FORMS - 1)]
}

/// Множитель цены формы.
pub fn form_value(form: u8) -> f64 {
    if form == FORM_STONE {
        return STONE_VALUE;
    }
    FORM_VALUE[(form as usize).min(N_FORMS - 1)]
}

/// Следующая стихия по кругу трансмутации: огонь -> земля -> вода -> эфир -> огонь.
pub fn next_element(mask: u8) -> u8 {
    match mask {
        1 => 4,
        4 => 2,
        2 => 8,
        _ => 1,
    }
}

pub fn mix_name(mask: u8) -> &'static str {
    MIX_NAMES[(mask & 15) as usize]
}

impl Item {
    pub fn new(mask: u8, purity: u8, form: u8) -> Item {
        Item { mask, purity, form, boxed: false }
    }
    pub fn base(elem: u8) -> Item {
        Item { mask: 1 << elem, purity: 1, form: 0, boxed: false }
    }
    /// Тот же предмет, уложенный в ящик / вынутый из ящика.
    pub fn packed(self, boxed: bool) -> Item {
        Item { boxed, ..self }
    }
    /// Сколько штук товара в этом предмете: ящик считается за CRATE_SIZE.
    pub fn pieces(self) -> u32 {
        if self.boxed {
            CRATE_SIZE
        } else {
            1
        }
    }
    /// Индекс вида предмета без учёта чистоты (так каталог хранился в старых сохранениях).
    pub fn kind_idx(self) -> usize {
        (self.mask as usize - 1) + 15 * self.form as usize
    }
    /// Предмет по его номеру Item::id().
    pub fn from_id(id: usize) -> Item {
        Item::new((id % 15) as u8 + 1, (id / 15 % 3) as u8 + 1, (id / 45) as u8)
    }
    /// Премия лавки за первую продажу этой позиции каталога (у каждой чистоты своя).
    pub fn first_sale_bonus(self) -> f64 {
        self.packed(false).value() * 10.0
    }
    /// Номер позиции каталога (смесь, чистота, форма) и он же индекс спрайта в Assets::items.
    pub fn id(self) -> usize {
        (self.mask as usize - 1) + 15 * (self.purity as usize - 1) + 45 * self.form as usize
    }
    pub fn value(self) -> f64 {
        let n = [2.0, 8.0, 30.0, 120.0][(self.mask.count_ones() as usize).clamp(1, 4) - 1];
        let p = [1.0, 3.0, 10.0][(self.purity as usize).clamp(1, 3) - 1];
        n * p * form_value(self.form) * self.pieces() as f64
    }
    pub fn name(self) -> String {
        let m = (self.mask & 15) as usize;
        let mut s = if self.form == 0 {
            MIX_NAMES[m].to_string()
        } else if self.form == FORM_STONE {
            STONE_NAME.to_string()
        } else {
            format!("{} {}", FORM_NAMES[(self.form as usize).min(N_FORMS - 1)], MIX_GEN[m])
        };
        s.push_str(["", " II", " III"][(self.purity as usize).clamp(1, 3) - 1]);
        if self.boxed {
            s = format!("ЯЩИК: {}", s);
        }
        s
    }
}

// ====================== условия отбора ======================

/// Условие фильтра и сортировочного узла: каждое из полей можно задать отдельно,
/// незаданное (0) подходит любому предмету. В настройке постройки занимает COND_BITS бит.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Cond {
    /// 0 - любая смесь, иначе маска стихий 1..15.
    pub mask: u8,
    /// 0 - любая чистота, иначе 1..3.
    pub purity: u8,
    /// 0 - любая форма, иначе номер формы + 1.
    pub form: u8,
    /// 0 - неважно, 1 - только россыпью, 2 - только в ящике.
    pub boxed: u8,
}

pub const COND_BITS: u32 = 13;
const COND_MASK: u64 = (1 << COND_BITS) - 1;

impl Cond {
    pub fn from_bits(v: u64) -> Cond {
        let c = Cond { mask: (v & 15) as u8, purity: (v >> 4 & 3) as u8, form: (v >> 6 & 31) as u8, boxed: (v >> 11 & 3) as u8 };
        // Мусор из файла или сети приводится к "неважно".
        Cond {
            form: if c.form as usize > N_FORMS_ALL { 0 } else { c.form },
            boxed: if c.boxed > 2 { 0 } else { c.boxed },
            ..c
        }
    }
    pub fn bits(self) -> u64 {
        self.mask as u64 | (self.purity as u64) << 4 | (self.form as u64) << 6 | (self.boxed as u64) << 11
    }
    /// Условие "ровно такой предмет" - так фильтр запоминает первый проехавший образец.
    pub fn exact(it: Item) -> Cond {
        Cond { mask: it.mask & 15, purity: it.purity.clamp(1, 3), form: it.form + 1, boxed: 1 + it.boxed as u8 }
    }
    pub fn is_any(self) -> bool {
        self.bits() == 0
    }
    pub fn matches(self, it: Item) -> bool {
        (self.mask == 0 || self.mask == it.mask)
            && (self.purity == 0 || self.purity == it.purity)
            && (self.form == 0 || self.form == it.form + 1)
            && (self.boxed == 0 || (self.boxed == 2) == it.boxed)
    }
    /// Предмет для значка на постройке: незаданные поля заменены самыми простыми.
    pub fn icon(self) -> Item {
        let mask = if self.mask == 0 { 15 } else { self.mask };
        Item { mask, purity: self.purity.max(1), form: self.form.saturating_sub(1), boxed: self.boxed == 2 }
    }
    /// Короткое описание: "СЛИТОК, ПАР, II" или "ВСЁ".
    pub fn text(self) -> String {
        let mut v: Vec<&str> = Vec::new();
        if self.form > 0 {
            v.push(form_name(self.form - 1));
        }
        if self.mask > 0 {
            v.push(MIX_NAMES[self.mask as usize]);
        }
        if self.purity > 0 {
            v.push(["I", "II", "III"][self.purity as usize - 1]);
        }
        if self.boxed > 0 {
            v.push(["РОССЫПЬЮ", "В ЯЩИКЕ"][self.boxed as usize - 1]);
        }
        if v.is_empty() {
            "ВСЁ".to_string()
        } else {
            v.join(", ")
        }
    }
}

/// Как получить форму: (название машины, из чего).
pub fn form_recipe(form: u8) -> (String, String) {
    if form == 0 {
        return ("ЭКСТРАКТОР, СМЕСИТЕЛЬ".to_string(), "ЖИЛЫ СТИХИЙ".to_string());
    }
    if form == FORM_STONE {
        return ("АТАНОР".to_string(), "МАГИСТЕРИИ ОГНЯ, ВОДЫ, ЗЕМЛИ, ЭФИРА".to_string());
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
    /// Делитель с пропорцией: N предметов в первый выход, затем M во второй.
    Ratio,
    /// Делитель по таймеру: всё идёт в один выход, выход меняется раз в SWITCH_PERIODS[уровень] секунд.
    Switch,
    /// Алтарь: сжигает изделия и даёт ауру машинам вокруг.
    Altar,
    /// Тигель: два изделия одной формы без общих стихий -> та же форма со слитой смесью.
    Crucible,
    /// Трансмутатор: две эссенции одной стихии -> эссенция следующей по кругу.
    Transmuter,
    /// Атанор: по магистерию каждой из четырёх чистых стихий -> философский камень.
    Opus,
    /// Упаковщик: CRATE_SIZE одинаковых предметов -> ящик. Распаковщик - обратно.
    Packer,
    Unpacker,
    /// Телепорт: передатчик шлёт предметы приёмникам с тем же кодом.
    TeleIn,
    TeleOut,
    /// Сортировочный узел: вход сзади, три выхода, у каждого своё условие.
    Sorter,
    /// Деталь слоя сигналов: провод, датчик, вентиль или исполнитель. Клеток не занимает.
    Logic(Lk),
    /// Гильдейская лавка: принимает товар как сама лавка алхимика (полная цена, контракты, заказы).
    GuildPost,
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

/// Детали слоя сигналов.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lk {
    Wire,
    /// Датчик затора: лента под ним стоит и забита (или у машины не уходит готовое).
    SenJam,
    /// Датчик склада: заполнен на N% и больше.
    SenStore,
    /// Датчик предмета: импульс на каждый въехавший предмет, подходящий под условие.
    SenItem,
    Timer,
    And,
    Or,
    Not,
    /// Защёлка: вход сзади включает, входы с боков сбрасывают.
    Latch,
    /// Счётчик до N: каждый N-й импульс сзади даёт импульс на выходе; входы с боков сбрасывают.
    Counter,
    /// Исполнитель: управляет постройкой под собой (ворота на ленте, машина, выход переключателя).
    Act,
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

pub const TAB_NAMES: [&str; 7] = ["ЛОГИСТИКА", "ЭССЕНЦИИ", "ОБРАБОТКА", "СБОРКА", "ШЕДЕВРЫ", "ТАИНСТВА", "ЛОГИКА"];

/// Все постройки. Индекс - постоянный идентификатор (он же пишется в сохранение), последняя - лавка.
pub const KINDS: [KindDef; 63] = [
    kd("КОНВЕЙЕР", "ВЕЗЁТ ПРЕДМЕТЫ. ТЯНИ МЫШЬЮ, ЧТОБЫ ПРОЛОЖИТЬ ЛИНИЮ.", 1.0, 0, Role::Belt(1.0), 0, 0x4A4E5A),
    kd("ЭКСТРАКТОР", "СТАВИТСЯ НА ЖИЛУ И КАЧАЕТ ЭССЕНЦИЮ СТИХИИ.", 20.0, 0, Role::Extractor(1.0), 1, 0x8A93A6),
    kd("СМЕСИТЕЛЬ", "СМЕШИВАЕТ ДВЕ ЭССЕНЦИИ БЕЗ ОБЩИХ СТИХИЙ В ОДНУ.", 60.0, 1, Role::Mixer, 1, 0x3FA7A0),
    kd("ДЕЛИТЕЛЬ", "ДЕЛИТ ПОТОК: ВПЕРЁД, ВПРАВО И ВЛЕВО ПО ОЧЕРЕДИ.", 25.0, 2, Role::Splitter, 0, 0xE0B43C),
    kd("МОСТ", "ДВА ПОТОКА ПЕРЕСЕКАЮТСЯ, НЕ СМЕШИВАЯСЬ.", 15.0, 2, Role::Bridge, 0, 0x9B6B43),
    kd("ДИСТИЛЛЯТОР", "ДВЕ ОДИНАКОВЫЕ ЭССЕНЦИИ > ОДНА БОЛЕЕ ЧИСТАЯ (X3 ЦЕНЫ). СТОРОНУ МОЖНО СДЕЛАТЬ ВЫХОДОМ ОТКАЗА.", 150.0, 3, Role::Distiller, 1, 0xC7773A),
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
    kd("ФИЛЬТР", "ПОДХОДЯЩЕЕ ПОД ОБРАЗЕЦ ИДЁТ ПРЯМО, ОСТАЛЬНОЕ ВБОК. E - ВЫБРАТЬ ОБРАЗЕЦ.", 80.0, 7, Role::Filter, 0, 0x8A5CC0),
    kd("СКЛАД", "КОПИТ ДО 100 ОДИНАКОВЫХ ПРЕДМЕТОВ И ВЫДАЁТ ВПЕРЁД.", 100.0, 6, Role::Storage, 0, 0x9B6B43),
    kd("УТИЛИЗАТОР", "УНИЧТОЖАЕТ ВСЁ, ЧТО В НЕГО ПРИВЕЗУТ.", 10.0, 0, Role::Trash, 0, 0x3A3346),
    kd("ТОРГОВЫЙ ПОСТ", "ПРОДАЁТ НА МЕСТЕ ЗА ПОЛЦЕНЫ. В КОНТРАКТЫ НЕ ИДЁТ.", 500.0, 12, Role::Trade, 0, 0xB8873A),
    kd("ПОДЗЕМНЫЙ КОНВЕЙЕР", "НЫРЯЕТ ПОД ПОСТРОЙКИ. ВХОД И ВЫХОД - В ОДНУ СТОРОНУ, НЕ ДАЛЬШЕ 6 КЛЕТОК.", 30.0, 4, Role::Tunnel, 0, 0x6A5A4A),
    kd("ПЕРЕКЛЮЧАТЕЛЬ", "ВСЁ ИДЁТ В ОДИН ВЫХОД, А ВЫХОД МЕНЯЕТСЯ ПО ТАЙМЕРУ. УРОВЕНЬ ЗАДАЁТ ПЕРИОД: 20, 40, 60, 120, 240 С.", 2000.0, 8, Role::Switch, 0, 0xC0508A),
    kd("ДЕЛИТЕЛЬ С ПРОПОРЦИЕЙ", "N ПРЕДМЕТОВ ВПЕРЁД, ЗАТЕМ M ВПРАВО. ПРОПОРЦИЯ ЗАДАЁТСЯ В НАСТРОЙКЕ (E).", 300.0, 6, Role::Ratio, 0, 0xD98A2E),
    kd("АЛТАРЬ", "СЖИГАЕТ ИЗДЕЛИЯ, ДАЁТ АУРУ ВОКРУГ: ОГОНЬ - СКОРОСТЬ, ЗЕМЛЯ - X2 ВЫХОД, ВОДА - ЭКОНОМИЯ, ЭФИР - РАДИУС.", 75000.0, 14, Role::Altar, 5, 0x7A3A5A),
    kd("ТИГЕЛЬ", "ДВА ИЗДЕЛИЯ ОДНОЙ ФОРМЫ БЕЗ ОБЩИХ СТИХИЙ > ТА ЖЕ ФОРМА СО СЛИТОЙ СМЕСЬЮ. ЧИСТОТА - МЕНЬШАЯ.", 25000.0, 12, Role::Crucible, 5, 0xB5562E),
    kd("ТРАНСМУТАТОР", "ДВЕ ЭССЕНЦИИ СТИХИИ > ОДНА СЛЕДУЮЩЕЙ ПО КРУГУ: ОГОНЬ > ЗЕМЛЯ > ВОДА > ЭФИР > ОГОНЬ.", 6000.0, 9, Role::Transmuter, 5, 0x4A7A8C),
    kd("АТАНОР", "МАГИСТЕРИИ ОГНЯ, ВОДЫ, ЗЕМЛИ И ЭФИРА > ФИЛОСОФСКИЙ КАМЕНЬ (ЦЕНА X2000). ЧИСТОТА - МЕНЬШАЯ.", 5000000.0, 19, Role::Opus, 5, 0x8A6A2A),
    kd("УПАКОВЩИК", "8 ОДИНАКОВЫХ ПРЕДМЕТОВ > ОДИН ЯЩИК. ЛАВКА ПРИНИМАЕТ ЯЩИК КАК 8 ПРОДАЖ.", 8000.0, 10, Role::Packer, 5, 0x9B6B43),
    kd("РАСПАКОВЩИК", "РАЗБИРАЕТ ЯЩИК ОБРАТНО НА 8 ПРЕДМЕТОВ.", 8000.0, 10, Role::Unpacker, 5, 0x7A5A3A),
    kd("ТЕЛЕПОРТ-ПЕРЕДАТЧИК", "КРОЛИЧЬЯ НОРА: ВСЁ, ЧТО ВОШЛО, ЧЕРЕЗ 1 С ВЫХОДИТ У ПРИЁМНИКОВ С ТЕМ ЖЕ КОДОМ (E).", 750000.0, 16, Role::TeleIn, 5, 0x5A3A8A),
    kd("ТЕЛЕПОРТ-ПРИЁМНИК", "ВЫДАЁТ ПРИСЛАННОЕ ПЕРЕДАТЧИКАМИ С ТЕМ ЖЕ КОДОМ (E). НЕСКОЛЬКО ПРИЁМНИКОВ - ПО ОЧЕРЕДИ.", 250000.0, 16, Role::TeleOut, 5, 0x3A6A8A),
    kd("СОРТИРОВОЧНЫЙ УЗЕЛ", "ОДИН ВХОД, ТРИ ВЫХОДА, У КАЖДОГО СВОЁ УСЛОВИЕ (E). ВЫХОД БЕЗ УСЛОВИЯ БЕРЁТ ОСТАЛЬНОЕ.", 1500.0, 9, Role::Sorter, 0, 0x4A8A6A),
    kd("ПРОВОД", "НЕСЁТ СИГНАЛ МЕЖДУ ДЕТАЛЯМИ ЛОГИКИ. СЛОЙ ПРОВОДОВ - КЛАВИША L, КЛЕТОК НЕ ЗАНИМАЕТ.", 2.0, 3, Role::Logic(Lk::Wire), 6, 0xC9A23A),
    kd("ДАТЧИК ЗАТОРА", "СИГНАЛ, ПОКА ЛЕНТА ПОД НИМ СТОИТ ЗАБИТАЯ (ИЛИ МАШИНА НЕ МОЖЕТ ВЫДАТЬ ГОТОВОЕ).", 150.0, 5, Role::Logic(Lk::SenJam), 6, 0xE8553A),
    kd("ДАТЧИК СКЛАДА", "СИГНАЛ, ПОКА СКЛАД ПОД НИМ ЗАПОЛНЕН НА N% И БОЛЬШЕ (E - ПОРОГ).", 150.0, 6, Role::Logic(Lk::SenStore), 6, 0x9B6B43),
    kd("ДАТЧИК ПРЕДМЕТА", "ИМПУЛЬС НА КАЖДЫЙ ПРЕДМЕТ, ВЪЕХАВШИЙ В ПОСТРОЙКУ ПОД НИМ И ПОДХОДЯЩИЙ ПОД УСЛОВИЕ (E).", 150.0, 5, Role::Logic(Lk::SenItem), 6, 0x8A5CC0),
    kd("ТАЙМЕР", "ПОЛОВИНУ ПЕРИОДА ВКЛЮЧЁН, ПОЛОВИНУ ВЫКЛЮЧЕН. ПЕРИОД ЗАДАЁТСЯ В НАСТРОЙКЕ (E).", 200.0, 5, Role::Logic(Lk::Timer), 6, 0xC0508A),
    kd("ВЕНТИЛЬ И", "ВЫХОД ВПЕРЁД ВКЛЮЧЁН, КОГДА ВКЛЮЧЕНЫ ВСЕ ПОДКЛЮЧЁННЫЕ ВХОДЫ (СЗАДИ И С БОКОВ).", 100.0, 6, Role::Logic(Lk::And), 6, 0x3D8FBF),
    kd("ВЕНТИЛЬ ИЛИ", "ВЫХОД ВПЕРЁД ВКЛЮЧЁН, КОГДА ВКЛЮЧЁН ХОТЯ БЫ ОДИН ВХОД.", 100.0, 6, Role::Logic(Lk::Or), 6, 0x4FA36A),
    kd("ВЕНТИЛЬ НЕ", "ВЫХОД ВПЕРЁД ВКЛЮЧЁН, КОГДА НИ ОДИН ВХОД НЕ ВКЛЮЧЁН.", 100.0, 6, Role::Logic(Lk::Not), 6, 0xB8478F),
    kd("ЗАЩЁЛКА", "ВХОД СЗАДИ ВКЛЮЧАЕТ ВЫХОД, ВХОД СБОКУ ВЫКЛЮЧАЕТ. МЕЖДУ НИМИ ВЫХОД ПОМНИТ СОСТОЯНИЕ.", 250.0, 7, Role::Logic(Lk::Latch), 6, 0xD9A82E),
    kd("СЧЁТЧИК", "КАЖДЫЙ N-Й ИМПУЛЬС СЗАДИ ДАЁТ ИМПУЛЬС ВПЕРЁД (E - ЧИСЛО N). ВХОД СБОКУ ОБНУЛЯЕТ СЧЁТ.", 250.0, 7, Role::Logic(Lk::Counter), 6, 0x6E8B3D),
    kd("ИСПОЛНИТЕЛЬ", "УПРАВЛЯЕТ ПОСТРОЙКОЙ ПОД СОБОЙ: ВОРОТА НА ЛЕНТЕ, ОСТАНОВ МАШИНЫ, ВЫХОД ПЕРЕКЛЮЧАТЕЛЯ (E - РЕЖИМ).", 300.0, 5, Role::Logic(Lk::Act), 6, 0xF0C24B),
    kd("ГИЛЬДЕЙСКАЯ ЛАВКА", "ФИЛИАЛ ЛАВКИ: ПОЛНАЯ ЦЕНА, КОНТРАКТЫ И ЗАКАЗЫ. ОТКРЫВАЕТСЯ ПРИ РЕПУТАЦИИ 10 В ЛЮБОЙ ГИЛЬДИИ.", 150000.0, 0, Role::GuildPost, 0, 0x8A2A4A),
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
    /// Настройка постройки, смысл зависит от роли: условие фильтра, пропорция делителя,
    /// сторона отказа машины. 0 - не задано. Пишется в сохранение седьмым числом.
    pub cfg: u64,
    /// Предмет, который машина не смогла взять и выдаёт через сторону отказа.
    pub rej: Option<Item>,
    /// Сколько ещё копий готового предмета выдать (двойной выход от ауры земли).
    pub extra: u8,
    /// Ячейки атанора по стихиям [огонь, вода, земля, эфир]: сколько магистериев лежит...
    pub bins: [u8; 4],
    /// ...и наименьшая чистота среди них.
    pub binp: [u8; 4],
    /// Накопители шансов от аур: [экономия сырья, двойной выход]. Шанс срабатывает, когда
    /// накопитель доходит до единицы, - так симуляция остаётся детерминированной.
    acc: [f32; 2],
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
            cfg: 0,
            rej: None,
            extra: 0,
            bins: [0; 4],
            binp: [0; 4],
            acc: [0.0; 2],
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
    matches!(role, Role::Splitter | Role::Overflow | Role::Filter | Role::Switch | Role::Ratio | Role::Sorter)
}

/// Машины, которым можно назначить сторону отказа: туда уходит то, что машина взять не может.
pub fn can_reject(role: Role) -> bool {
    matches!(role, Role::Distiller | Role::Crucible | Role::Transmuter | Role::Opus | Role::Packer | Role::Unpacker)
}

/// Сколько секунд предмет летит через телепорт. Пропускная способность не ограничена:
/// это кроличья нора - всё, что вошло, через секунду выходит у приёмника.
pub const TELE_PERIOD: f32 = 1.0;
/// Сколько предметов может одновременно лететь от одного передатчика (запас на случай, если приёмники заняты).
const TELE_QUEUE: usize = 64;
/// Кодов телепорта: 0000..9999.
pub const TELE_CODES: usize = 10_000;

/// Код телепорта из настройки постройки; None - код ещё не задан.
pub fn tele_code(cfg: u64) -> Option<usize> {
    if (1..=TELE_CODES as u64).contains(&cfg) {
        Some(cfg as usize - 1)
    } else {
        None
    }
}

pub fn tele_cfg(code: usize) -> u64 {
    (code % TELE_CODES) as u64 + 1
}

/// Условие выхода сортировочного узла: 0 - вперёд, 1 - вправо, 2 - влево.
pub fn sorter_cond(cfg: u64, k: usize) -> Cond {
    Cond::from_bits(cfg >> (COND_BITS * k as u32) & COND_MASK)
}

pub fn sorter_set(cfg: u64, k: usize, c: Cond) -> u64 {
    let sh = COND_BITS * k as u32;
    (cfg & !(COND_MASK << sh)) | c.bits() << sh
}

/// Сколько магистериев одной стихии помещается в ячейку атанора.
pub const OPUS_CELL: u8 = 4;
/// Секунды на один философский камень.
const OPUS_TIME: f32 = 8.0;

/// Сторона отказа постройки (номер относительно направления), если она назначена.
pub fn reject_rel(b: &Building) -> Option<usize> {
    let r = (b.cfg & 7) as usize;
    if can_reject(b.kind.role()) && (1..=4).contains(&r) {
        Some(r - 1)
    } else {
        None
    }
}

/// Пропорция делителя (N, M); незаданная настройка - 1:1.
pub fn ratio_nm(cfg: u64) -> (u32, u32) {
    (((cfg & 15) as u32).clamp(1, 9), ((cfg >> 4 & 15) as u32).clamp(1, 9))
}

pub fn ratio_cfg(n: u32, m: u32) -> u64 {
    (n.clamp(1, 9) | m.clamp(1, 9) << 4) as u64
}

/// Есть ли у постройки окно настройки (клавиша E или щелчок по центру).
pub fn has_panel(kind: Kind) -> bool {
    matches!(kind.role(), Role::Filter | Role::Ratio | Role::Sorter | Role::TeleIn | Role::TeleOut)
}

/// Приводит настройку из файла или сети к допустимой для этой роли.
pub fn clean_cfg(role: Role, v: u64) -> u64 {
    match role {
        Role::Filter => Cond::from_bits(v & COND_MASK).bits(),
        Role::Ratio if v != 0 => {
            let (n, m) = ratio_nm(v);
            ratio_cfg(n, m)
        }
        Role::Sorter => (0..3).fold(0, |acc, k| sorter_set(acc, k, sorter_cond(v, k))),
        Role::TeleIn | Role::TeleOut if tele_code(v).is_some() => v,
        r if can_reject(r) && (v & 7) <= 4 => v & 7,
        _ => 0,
    }
}

/// Сколько секунд переключатель держит один выход, по уровням 1..5.
pub const SWITCH_PERIODS: [f64; MAX_LEVEL as usize] = [20.0, 40.0, 60.0, 120.0, 240.0];

pub fn switch_period(level: u8) -> f64 {
    SWITCH_PERIODS[(level.clamp(1, MAX_LEVEL) - 1) as usize]
}

/// Сколько режимов у сторон постройки: 0 - не настраивается, 3 - добыча (без входа), 4 - все.
pub fn port_modes(kind: Kind) -> u8 {
    match kind.role() {
        // Приёмник телепорта, как и добыча, только выдаёт; у узла стороны закреплены за условиями.
        Role::Extractor(_) | Role::TeleOut => 3,
        Role::Sorter => 0,
        Role::Packer | Role::Unpacker => 4,
        Role::Mixer | Role::Distiller | Role::Convert(..) | Role::Assemble(..) | Role::Storage => 4,
        Role::Crucible | Role::Transmuter | Role::Opus => 4,
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
        PORT_AUTO => match b.kind.role() {
            // У делителя с пропорцией два выхода: вперёд и вправо.
            Role::Ratio => rel < 2,
            r if splitter_like(r) => rel != 2,
            _ => rel == 0 && !b.ports.contains(&PORT_OUT),
        },
        _ => false,
    }
}

/// Принимает ли постройка предметы с этой стороны.
pub fn side_in(b: &Building, rel: usize) -> bool {
    let role = b.kind.role();
    if matches!(role, Role::Extractor(_) | Role::TeleOut) {
        return false;
    }
    match b.ports[rel] {
        PORT_IN => true,
        PORT_AUTO => match role {
            Role::Ratio => rel >= 2,
            r if splitter_like(r) => rel == 2,
            _ => rel != 0,
        },
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

// ====================== алтари и ауры ======================

/// Сколько секунд алтарь сжигает одно изделие.
pub const ALTAR_BURN: f32 = 30.0;
/// Радиус ауры без эфира: квадрат 7x7 вокруг алтаря.
pub const ALTAR_RADIUS: i32 = 3;
/// Потолок радиуса с любым эфиром.
pub const ALTAR_RADIUS_MAX: i32 = 9;

/// Сила аур изделия по стихиям [огонь, вода, земля, эфир]; 0 - стихии в смеси нет.
/// Форма задаёт силу (чем дороже форма, тем сильнее), чистота - множитель x1 / x1.5 / x2,
/// а смесь из нескольких стихий делит силу между своими аурами.
pub fn aura_of(it: Item) -> [f32; 4] {
    if it.form == 0 || it.boxed || it.mask & 15 == 0 {
        return [0.0; 4];
    }
    let form = 0.05 * form_value(it.form).log2() as f32;
    let purity = [1.0, 1.5, 2.0][(it.purity as usize).clamp(1, 3) - 1];
    let share = [1.0, 0.6, 0.45, 0.35][it.mask.count_ones() as usize - 1];
    let s = form * purity * share;
    [0, 1, 2, 3].map(|e| if it.mask >> e & 1 == 1 { s } else { 0.0 })
}

/// На сколько клеток аура эфира такой силы расширяет радиус алтаря.
pub fn aura_radius_bonus(s: f32) -> i32 {
    ((s * 6.0).round() as i32).clamp(0, ALTAR_RADIUS_MAX - ALTAR_RADIUS)
}

/// Шанс двойного выхода (земля) и экономии сырья (вода) при силе ауры `s`.
pub fn aura_chance(s: f32) -> f32 {
    (0.5 * s).clamp(0.0, 0.5)
}

/// Действуют ли ауры на постройку: только на то, что добывает или перерабатывает.
pub fn aura_affects(role: Role) -> bool {
    matches!(
        role,
        Role::Extractor(_)
            | Role::Mixer
            | Role::Distiller
            | Role::Convert(..)
            | Role::Assemble(..)
            | Role::Crucible
            | Role::Transmuter
            | Role::Opus
    )
}

/// Работающий алтарь: где стоит, какие ауры даёт и на какой радиус.
pub struct AltarInfo {
    pub x: i32,
    pub y: i32,
    pub s: [f32; 4],
    pub r: i32,
}

/// Накопитель шанса: прибавляет `p` и срабатывает, когда набралась единица.
fn bump(acc: &mut f32, p: f32) -> bool {
    if p <= 0.0 {
        return false;
    }
    *acc += p;
    if *acc >= 1.0 {
        *acc -= 1.0;
        true
    } else {
        false
    }
}

// ====================== рынок, гильдии, мегапроект, виток ======================

/// Насыщение рынка. Каждая проданная штука добавляет позиции единицу "затоваренности",
/// та убывает по экспоненте с постоянной MARKET_TAU секунд. Цена = 1 / (1 + затоваренность / MARKET_HALF),
/// но не ниже MARKET_FLOOR. При ровном потоке r шт/с затоваренность стремится к r * MARKET_TAU,
/// то есть цена падает вдвое при 0.5 шт/с одной позиции, а выручка с позиции упирается в потолок.
pub const MARKET_TAU: f64 = 120.0;
pub const MARKET_HALF: f64 = 60.0;
pub const MARKET_FLOOR: f64 = 0.25;

/// Множитель цены при данной затоваренности.
pub fn market_curve(glut: f64) -> f64 {
    (1.0 / (1.0 + glut.max(0.0) / MARKET_HALF)).max(MARKET_FLOOR)
}

pub const GUILD_NAMES: [&str; 4] = ["ГИЛЬДИЯ ОГНЯ", "ГИЛЬДИЯ ВОДЫ", "ГИЛЬДИЯ ЗЕМЛИ", "ГИЛЬДИЯ ЭФИРА"];
/// Пороги репутации: скин лент, жила чистоты II, особая постройка.
pub const REP_SKIN: u32 = 3;
pub const REP_VEIN: u32 = 6;
pub const REP_BUILD: u32 = 10;
/// Награда за завершённое Великое Делание.
pub const GREAT_REWARD: f64 = 5e9;
/// Бонусы витка за каждый философский камень (не больше STONES_CAP камней в счёт).
pub const STONE_PRICE_BONUS: f64 = 0.05;
pub const STONE_SPEED_BONUS: f32 = 0.02;
pub const STONE_START_MONEY: f64 = 500.0;
pub const STONES_CAP: u32 = 100;
/// Цена сноса одной клетки преграды.
pub const ROCK_COST: f64 = 2000.0;

/// Срочный заказ гильдии.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Order {
    pub guild: u8,
    pub item: Item,
    pub need: u32,
    pub reward: f64,
    pub rep: u32,
    /// Сколько секунд даётся на заказ.
    pub secs: f64,
}

pub const ACH_NAMES: [(&str, &str); 12] = [
    ("ACH_FIRST_CONTRACT", "ПЕРВЫЙ КОНТРАКТ"),
    ("ACH_MAGNATE", "МАГИСТЕРИЙ: ФИНАЛЬНЫЙ КОНТРАКТ"),
    ("ACH_STONE", "ФИЛОСОФСКИЙ КАМЕНЬ"),
    ("ACH_CATALOG_100", "КАТАЛОГ: 100 ПОЗИЦИЙ"),
    ("ACH_CATALOG_HALF", "КАТАЛОГ: ПОЛОВИНА"),
    ("ACH_CATALOG_FULL", "КАТАЛОГ СОБРАН"),
    ("ACH_GREAT_WORK", "ВЕЛИКОЕ ДЕЛАНИЕ"),
    ("ACH_GUILD_ORDER", "ПЕРВЫЙ ЗАКАЗ ГИЛЬДИИ"),
    ("ACH_GUILD_FRIEND", "РЕПУТАЦИЯ 10 В ГИЛЬДИИ"),
    ("ACH_NEW_CYCLE", "НОВЫЙ ВИТОК"),
    ("ACH_LOGIC", "ПЕРВАЯ СХЕМА: ИСПОЛНИТЕЛЬ СРАБОТАЛ"),
    ("ACH_RIFT", "ДОБЫЧА ИЗ РАЗЛОМА"),
];

// ====================== слой сигналов ======================

/// Логика считается с постоянной частотой, независимо от частоты кадров.
pub const LOGIC_HZ: f32 = 60.0;

/// Деталь слоя сигналов на тайле.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Node {
    pub kind: Kind,
    /// Куда смотрит выход вентиля, датчика и таймера.
    pub dir: Dir,
    /// Настройка: порог склада в %, условие датчика предмета, период таймера в десятых долях
    /// секунды, число N счётчика, режим исполнителя (0 - сигнал останавливает, 1 - сигнал включает).
    pub param: u64,
    /// Выход детали после последнего такта логики.
    pub out: bool,
    /// Счёт счётчика, такты таймера, очередь импульсов датчика предмета.
    pub count: u32,
    /// Был ли включён счётный вход на прошлом такте (счётчик считает фронты).
    pub prev: bool,
}

impl Node {
    pub fn lk(&self) -> Lk {
        match self.kind.role() {
            Role::Logic(k) => k,
            _ => Lk::Wire,
        }
    }
}

/// Настройка детали по умолчанию и её допустимые значения.
pub fn clean_lparam(lk: Lk, v: u64) -> u64 {
    match lk {
        Lk::SenStore => v.clamp(1, 100),
        Lk::SenItem => Cond::from_bits(v & COND_MASK).bits(),
        Lk::Timer => v.clamp(2, 36_000),
        Lk::Counter => v.clamp(1, 9999),
        Lk::Act => v & 1,
        _ => 0,
    }
}

fn default_lparam(lk: Lk) -> u64 {
    match lk {
        Lk::SenStore => 100,
        Lk::Timer => 20,
        Lk::Counter => 8,
        _ => 0,
    }
}

/// Есть ли у детали настройка (окно по клавише E).
pub fn lk_has_param(lk: Lk) -> bool {
    matches!(lk, Lk::SenStore | Lk::SenItem | Lk::Timer | Lk::Counter | Lk::Act)
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

/// За сколько последних минут экран статистики помнит продажи.
pub const STAT_MINUTES: usize = 5;

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
    /// Каталог: какие позиции уже продавались, по Item::id() - каждая чистота отдельно.
    /// Первые N_ITEM_IDS - сетка каталога, дальше - философский камень.
    pub discovered: Vec<bool>,
    /// Новинки каталога с премией, ещё не показанные игроку.
    pub pending_new: Vec<(Item, f64)>,
    /// Копия чужого мира в сетевой игре: продажи не меняют экономику, её присылает хост.
    pub replica: bool,
    /// Сколько штук каждого предмета (по Item::id(), с учётом чистоты) продано в лавке.
    /// Чистая статистика для проверок: ни на что не влияет и в сохранение не пишется.
    pub sold_by_id: Vec<u32>,
    /// Продажи в лавке по минутам (кольцо из STAT_MINUTES вёдер, в каждом - штуки по Item::id()).
    stat: Vec<Vec<u32>>,
    stat_cur: usize,
    /// Номер минуты мира, которой соответствует текущее ведро.
    stat_min: i64,
    /// Настройки, которые постройки поменяли сами (фильтр запомнил образец): хост рассылает их клиентам.
    pub pending_cfg: Vec<(i32, i32, u64)>,
    /// Сила аур на каждом тайле: [огонь - скорость, вода - экономия сырья, земля - двойной выход].
    /// От нескольких алтарей берётся сильнейшая аура каждого вида, они не складываются.
    aura: Vec<[f32; 3]>,
    aura_on: bool,
    /// Затоваренность рынка по позициям (Item::id()) и время мира, на которое она посчитана.
    glut: Vec<f32>,
    glut_t: Vec<f64>,
    /// Великое Делание: какие позиции каталога уже сданы в лавку.
    pub great: Vec<bool>,
    pub great_done: bool,
    /// Виток: номер, философские камни прошлых витков (дают бонусы) и сданные в этом витке.
    pub cycle: u32,
    pub stones: u32,
    pub stones_now: u32,
    /// Гильдии: репутация, три предложения, взятый заказ (с числом сданного и сроком).
    pub rep: [u32; 4],
    pub offers: Vec<Order>,
    pub order: Option<(Order, u32, f64)>,
    order_seq: u32,
    offers_t: f64,
    /// Скин лент: 0 - обычный, 1..4 - цвета гильдии (нужна репутация REP_SKIN).
    pub skin: u8,
    /// Достижения (биты по ACH_NAMES) и только что полученные - для сообщения игроку.
    pub ach: u32,
    pub pending_ach: Vec<usize>,
    /// Сообщения игроку о событиях мира (заказ выполнен, награда гильдии и т.п.).
    pub pending_msgs: Vec<String>,
    /// Карта: жилы чистоты II (разломы и дар гильдии), преграды и снесённые преграды.
    pub rich: Vec<bool>,
    pub rocks: Vec<bool>,
    cleared: Vec<usize>,
    /// Версия карты: 0 - как в старых сохранениях, 2 - с разломами и преградами.
    pub map_ver: u8,
    /// Слой сигналов: деталь логики на тайле (поверх построек, клеток не занимает).
    pub logic: Vec<Option<Node>>,
    /// Под напряжением ли провод на тайле (после последнего такта логики).
    pub powered: Vec<bool>,
    /// Постройка на тайле остановлена исполнителем.
    halted: Vec<bool>,
    /// Выход переключателя, выбранный исполнителем: 0 - по таймеру, 1 и 2 - первый и второй.
    sw_sel: Vec<u8>,
    n_logic: usize,
    ach_logic: bool,
    /// Датчик сработал так, как копия мира предсказать не может (предмет, затор, склад):
    /// хост по этому флагу рассылает состояние логики вне очереди.
    pub logic_dirty: bool,
    logic_acc: f32,
    /// Сколько тактов логики прошло.
    pub logic_ticks: u64,
    /// Приёмники телепорта: (код, номер тайла), по возрастанию.
    tele_rx: Vec<(u16, usize)>,
    /// Передатчики в том же виде и тайл того, кто отдал предмет последним: когда приёмники
    /// заняты, свободный выход достаётся передатчикам кода по кругу, а не первому по порядку.
    tele_tx: Vec<(u16, usize)>,
    tele_last: Vec<u32>,
    /// Какому по счёту приёмнику канала достанется следующий предмет.
    tele_rr: Vec<u16>,
    /// Постройки или алтари изменились: ауры пересчитываются перед следующим шагом.
    dirty: bool,
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
            discovered: vec![false; N_ALL_IDS],
            stat: vec![vec![0; N_ALL_IDS]; STAT_MINUTES],
            stat_cur: 0,
            stat_min: 0,
            pending_new: Vec::new(),
            replica: false,
            sold_by_id: vec![0; N_ITEM_IDS],
            pending_cfg: Vec::new(),
            aura: vec![[0.0; 3]; n],
            aura_on: false,
            glut: vec![0.0; N_ALL_IDS],
            glut_t: vec![0.0; N_ALL_IDS],
            great: vec![false; N_ITEM_IDS],
            great_done: false,
            cycle: 0,
            stones: 0,
            stones_now: 0,
            rep: [0; 4],
            offers: Vec::new(),
            order: None,
            order_seq: 0,
            offers_t: 0.0,
            skin: 0,
            ach: 0,
            pending_ach: Vec::new(),
            pending_msgs: Vec::new(),
            rich: vec![false; n],
            rocks: vec![false; n],
            cleared: Vec::new(),
            map_ver: 0,
            logic: vec![None; n],
            powered: vec![false; n],
            halted: vec![false; n],
            sw_sel: vec![0; n],
            n_logic: 0,
            ach_logic: false,
            logic_dirty: false,
            logic_acc: 0.0,
            logic_ticks: 0,
            tele_rx: Vec::new(),
            tele_tx: Vec::new(),
            tele_last: vec![u32::MAX; TELE_CODES],
            tele_rr: vec![0; TELE_CODES],
            dirty: true,
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

    /// Новая игра: карта с разломами (редкие жилы чистоты II вдали от лавки) и преградами.
    /// Старые сохранения создаются через `new` и остаются с прежней картой.
    pub fn new_game(seed: u64) -> World {
        let mut w = World::new(seed);
        w.map_ver = 2;
        w.gen_features();
        w
    }

    /// Разломы и преграды. Свой генератор случайных чисел: расположение обычных жил не меняется.
    fn gen_features(&mut self) {
        let mut rng = Rng::new(self.seed ^ 0x5EED_F00D_CAFE);
        let (cx, cy) = (HUB_X + 1, HUB_Y + 1);
        let dist = |x: i32, y: i32| (x - cx).abs().max((y - cy).abs());
        for _ in 0..46 {
            let (x, y, n) = (rng.range(2, W - 2), rng.range(2, H - 2), rng.range(3, 10));
            let mut cells = vec![(x, y)];
            for _ in 0..n * 3 {
                let (bx, by) = cells[rng.range(0, cells.len() as i32) as usize];
                let d = Dir::from_u8(rng.range(0, 4) as u8);
                cells.push((bx + d.dx(), by + d.dy()));
            }
            for (x, y) in cells.into_iter().take(n as usize * 2) {
                if Self::inb(x, y) && dist(x, y) >= 8 && self.vein(x, y) == 0 {
                    self.rocks[(y * W + x) as usize] = true;
                }
            }
        }
        let mut made = 0;
        for _ in 0..200 {
            if made >= 6 {
                break;
            }
            let (x, y, e) = (rng.range(3, W - 3), rng.range(3, H - 3), rng.range(1, 5) as u8);
            if dist(x, y) < 26 {
                continue;
            }
            made += 1;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0)].into_iter().take(rng.range(3, 6) as usize) {
                let i = ((y + dy) * W + x + dx) as usize;
                self.veins[i] = e;
                self.rich[i] = true;
                self.rocks[i] = false;
            }
        }
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
        (1.0 + 0.3 * self.upgrades[2] as f32) * (1.0 + STONE_SPEED_BONUS * self.stones.min(STONES_CAP) as f32)
    }
    pub fn price_mult(&self) -> f64 {
        (1.0 + 0.15 * self.upgrades[3] as f64) * (1.0 + STONE_PRICE_BONUS * self.stones.min(STONES_CAP) as f64)
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
        if self.rocks[(y * W + x) as usize] {
            return Err("ПРЕГРАДА: ОБОЙДИ ИЛИ СНЕСИ ЕЁ (ПКМ) ЗА $2000");
        }
        if matches!(kind.role(), Role::Extractor(_)) && self.vein(x, y) == 0 {
            return Err("ДОБЫЧА СТАВИТСЯ ТОЛЬКО НА ЖИЛУ");
        }
        Ok(())
    }

    /// Ok(true) - что-то изменилось, Ok(false) - уже стоит то же самое.
    pub fn place(&mut self, kind: Kind, x: i32, y: i32, dir: Dir) -> Result<bool, &'static str> {
        if let Role::Logic(lk) = kind.role() {
            return self.place_logic(kind, lk, x, y, dir);
        }
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
        self.dirty = true;
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
        if self.rocks[idx] {
            // Преграду сносят за деньги.
            if self.money < ROCK_COST {
                return false;
            }
            self.money -= ROCK_COST;
            self.rocks[idx] = false;
            self.cleared.push(idx);
            return true;
        }
        match &self.tiles[idx] {
            Some(b) if b.kind != Kind::Hub => {
                self.money += self.refund(b.kind, b.level);
                self.tiles[idx] = None;
                self.dirty = true;
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
            Role::Extractor(_)
                | Role::Mixer
                | Role::Distiller
                | Role::Convert(..)
                | Role::Assemble(..)
                | Role::Switch
                | Role::Crucible
                | Role::Transmuter
                | Role::Opus
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
                // Сторона отказа обязана быть закрытой; если её открыли - отказ снимается.
                if reject_rel(b).map_or(false, |r| b.ports[r] != PORT_OFF) {
                    b.cfg &= !7;
                    b.rej = None;
                }
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
                b.cfg = 0;
                true
            }
            _ => false,
        }
    }

    /// Задаёт настройку постройки (условие, пропорцию, сторону отказа). false - настраивать нечего.
    pub fn set_cfg(&mut self, x: i32, y: i32, cfg: u64) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        self.dirty = true;
        match self.tiles[(y * W + x) as usize].as_mut() {
            Some(b) if has_panel(b.kind) || can_reject(b.kind.role()) => {
                let v = clean_cfg(b.kind.role(), cfg);
                if v != b.cfg {
                    b.cfg = v;
                    // Счётчик пропорции начинает заново, отказ без стороны пропадает.
                    if b.kind.role() == Role::Ratio {
                        b.count = 0;
                    }
                    b.rej = None;
                }
                if let Some(r) = reject_rel(b) {
                    b.ports[r] = PORT_OFF;
                }
                true
            }
            _ => false,
        }
    }

    /// Что станет со стороной `rel` по щелчку: (стороны, настройка). У машин с отказом
    /// круг длиннее: авто -> вход -> выход -> закрыто -> отказ -> авто.
    pub fn next_side(&self, x: i32, y: i32, rel: usize) -> Option<(u8, u64)> {
        let b = self.get(x, y)?;
        if !can_reject(b.kind.role()) {
            return self.next_ports(x, y, rel).map(|p| (p, b.cfg));
        }
        let mut p = b.ports;
        let mut cfg = b.cfg;
        if reject_rel(b) == Some(rel) {
            cfg &= !7;
            p[rel] = PORT_AUTO;
        } else if p[rel] == PORT_OFF {
            cfg = (cfg & !7) | (rel as u64 + 1);
        } else {
            p[rel] += 1;
        }
        Some((pack_ports(p), cfg))
    }

    // ---------- сетевая копия ----------

    /// Ставит постройку без проверок и денег (так клиент повторяет решения хоста).
    pub fn force_set(&mut self, kind: Kind, x: i32, y: i32, dir: Dir) {
        if !Self::inb(x, y) || kind == Kind::Hub {
            return;
        }
        if let Role::Logic(lk) = kind.role() {
            self.set_node(x, y, kind, lk, dir);
            return;
        }
        let idx = (y * W + x) as usize;
        match self.tiles[idx].as_mut() {
            Some(b) if b.kind == Kind::Hub => {}
            Some(b) if b.kind == kind => b.dir = dir,
            _ => self.tiles[idx] = Some(Building::new(kind, dir)),
        }
        self.dirty = true;
    }

    pub fn force_clear(&mut self, x: i32, y: i32) {
        if Self::inb(x, y) {
            let idx = (y * W + x) as usize;
            if self.rocks[idx] {
                self.rocks[idx] = false;
                self.cleared.push(idx);
                return;
            }
            if self.tiles[idx].as_ref().map_or(false, |b| b.kind != Kind::Hub) {
                self.tiles[idx] = None;
                self.dirty = true;
            }
        }
    }

    /// Принимает экономику от хоста; новые уровни и новинки каталога превращает в события.
    pub fn apply_state(&mut self, money: f64, level: usize, delivered: u32, upgrades: [u8; N_UPGRADES], found: &[bool]) {
        if level > self.level {
            self.pending_levelups += 1;
        }
        for i in 0..N_ALL_IDS.min(found.len()) {
            if found[i] && !self.discovered[i] {
                self.discovered[i] = true;
                let it = Item::from_id(i);
                self.pending_new.push((it, it.first_sale_bonus()));
            }
        }
        self.money = money;
        self.level = level;
        self.delivered = delivered;
        for i in 0..N_UPGRADES {
            self.upgrades[i] = upgrades[i].min(UPGRADE_MAX[i]);
        }
    }

    // ---------- рынок ----------

    /// Затоваренность позиции на текущий момент.
    pub fn glut_now(&self, id: usize) -> f64 {
        let g = self.glut[id] as f64;
        if g <= 0.0 {
            return 0.0;
        }
        g * (-((self.time - self.glut_t[id]).max(0.0)) / MARKET_TAU).exp()
    }

    /// Множитель цены позиции из-за насыщения рынка: 1.0 - рынок пуст.
    pub fn market_factor(&self, item: Item) -> f64 {
        market_curve(self.glut_now(item.id().min(N_ALL_IDS - 1)))
    }

    // ---------- открытие построек, достижения ----------

    /// Открыта ли постройка: обычные - по числу контрактов, гильдейская - по репутации.
    pub fn unlocked(&self, kind: Kind) -> bool {
        match kind.role() {
            Role::GuildPost => self.rep.iter().any(|r| *r >= REP_BUILD),
            _ => self.level >= kind.unlock_level(),
        }
    }

    fn award(&mut self, i: usize) {
        if self.ach >> i & 1 == 0 && !self.replica {
            self.ach |= 1 << i;
            self.pending_ach.push(i);
        }
    }

    // ---------- гильдии ----------

    /// Три новых предложения гильдий - по силам текущему уровню игрока.
    fn gen_offers(&mut self) {
        let mut rng = Rng::new(self.seed ^ (self.order_seq as u64 + 1).wrapping_mul(0x9E37_79B9));
        let level = self.level;
        let mut forms: Vec<u8> = vec![0];
        for d in KINDS.iter().filter(|d| d.unlock <= level) {
            match d.role {
                Role::Convert(_, to, _) | Role::Assemble(_, _, to, _) => forms.push(to),
                _ => {}
            }
        }
        self.offers.clear();
        for _ in 0..3 {
            let guild = rng.range(0, 4) as u8;
            let mut mask = 1u8 << guild;
            if level >= 1 && rng.range(0, 2) == 0 {
                mask |= 1 << rng.range(0, 4);
            }
            let purity = if level >= 3 && rng.range(0, 3) == 0 { 2 } else { 1 };
            let item = Item::new(mask, purity, forms[rng.range(0, forms.len() as i32) as usize]);
            let need = 15 + rng.range(0, 6) as u32 * 5;
            let hard = (purity > 1) as u32 + (mask.count_ones() > 1) as u32 + (item.form > 0) as u32;
            self.offers.push(Order {
                guild,
                item,
                need,
                reward: (item.value() * need as f64 * 6.0 + 150.0).round(),
                rep: 1 + hard / 2,
                secs: 240.0 + 60.0 * hard as f64 + 4.0 * need as f64,
            });
        }
        self.offers_t = self.time;
    }

    /// Берёт одно из трёх предложений; остальные два пропадают, появятся новые.
    pub fn take_order(&mut self, i: usize) -> Result<(), &'static str> {
        if self.order.is_some() {
            return Err("СНАЧАЛА ЗАКОНЧИ ТЕКУЩИЙ ЗАКАЗ");
        }
        let o = *self.offers.get(i).ok_or("НЕТ ТАКОГО ЗАКАЗА")?;
        self.order = Some((o, 0, self.time + o.secs));
        self.order_seq += 1;
        self.gen_offers();
        Ok(())
    }

    pub fn set_skin(&mut self, skin: u8) -> bool {
        if skin == 0 || (skin <= 4 && self.rep[skin as usize - 1] >= REP_SKIN) {
            self.skin = skin;
            true
        } else {
            false
        }
    }

    /// Награды за репутацию: при пересечении порога.
    fn add_rep(&mut self, g: usize, n: u32) {
        let old = self.rep[g];
        self.rep[g] = old + n;
        let crossed = |t: u32| old < t && old + n >= t;
        if crossed(REP_SKIN) {
            self.pending_msgs.push(format!("{}: ОТКРЫТ СКИН ЛЕНТ (ЭКРАН ГИЛЬДИЙ - G)", GUILD_NAMES[g]));
        }
        if crossed(REP_VEIN) {
            let n = self.enrich_vein(g as u8 + 1);
            self.pending_msgs.push(format!("{}: ЖИЛА У ЛАВКИ СТАЛА ЧИСТОТЫ II ({} КЛЕТОК)", GUILD_NAMES[g], n));
        }
        if crossed(REP_BUILD) {
            self.pending_msgs.push(format!("{}: ОТКРЫТА ГИЛЬДЕЙСКАЯ ЛАВКА (ВКЛАДКА ЛОГИСТИКА)", GUILD_NAMES[g]));
            self.award(8);
        }
    }

    /// Дар гильдии: ближайшая к лавке жила стихии целиком становится чистоты II.
    fn enrich_vein(&mut self, elem: u8) -> usize {
        let (cx, cy) = (HUB_X + 1, HUB_Y + 1);
        let start = (0..self.veins.len())
            .filter(|i| self.veins[*i] == elem && !self.rich[*i])
            .min_by_key(|i| ((*i as i32 % W - cx).abs().max((*i as i32 / W - cy).abs()), *i));
        let mut stack: Vec<usize> = start.into_iter().collect();
        let mut n = 0;
        while let Some(i) = stack.pop() {
            if self.rich[i] || self.veins[i] != elem {
                continue;
            }
            self.rich[i] = true;
            n += 1;
            for d in [Dir::N, Dir::E, Dir::S, Dir::W] {
                let (x, y) = (i as i32 % W + d.dx(), i as i32 / W + d.dy());
                if Self::inb(x, y) {
                    stack.push((y * W + x) as usize);
                }
            }
        }
        n
    }

    /// Раз в кадр: срок заказа, обновление предложений.
    fn guild_update(&mut self) {
        if self.replica {
            return;
        }
        if let Some((o, _, deadline)) = self.order {
            if self.time > deadline {
                self.order = None;
                self.pending_msgs.push(format!("{}: ЗАКАЗ ПРОСРОЧЕН", GUILD_NAMES[o.guild as usize]));
            }
        }
        // Предложения обновляются сами раз в 10 минут, чтобы не застаивались.
        if self.offers.len() < 3 || self.time - self.offers_t > 600.0 {
            self.order_seq += 1;
            self.gen_offers();
        }
    }

    // ---------- новый виток ----------

    /// Сброс фабрики: философские камни, сданные в этом витке, становятся постоянными бонусами.
    /// Остаются каталог, Великое Делание, репутация гильдий, достижения и сама карта.
    pub fn prestige(&mut self) -> Result<u32, &'static str> {
        if self.stones_now == 0 {
            return Err("НУЖЕН ХОТЯ БЫ ОДИН ФИЛОСОФСКИЙ КАМЕНЬ, СДАННЫЙ В ЛАВКУ");
        }
        let gained = self.stones_now;
        self.stones += gained;
        self.stones_now = 0;
        self.cycle += 1;
        for (i, t) in self.tiles.iter_mut().enumerate() {
            if t.as_ref().map_or(false, |b| b.kind != Kind::Hub) {
                *t = None;
            }
            self.logic[i] = None;
            self.halted[i] = false;
            self.sw_sel[i] = 0;
            self.powered[i] = false;
        }
        self.n_logic = 0;
        self.money = 200.0 + STONE_START_MONEY * self.stones.min(STONES_CAP) as f64;
        self.level = 0;
        self.delivered = 0;
        self.upgrades = [0; N_UPGRADES];
        self.glut.iter_mut().for_each(|g| *g = 0.0);
        self.order = None;
        self.order_seq += 1;
        self.gen_offers();
        self.dirty = true;
        self.award(9);
        Ok(gained)
    }

    // ---------- экономика одной строкой: для сохранения и для клиента в сети ----------

    /// Рынок, мегапроект, виток, гильдии и достижения в виде строк сохранения.
    pub fn econ_save(&self) -> String {
        let mut s = String::from("S");
        for id in 0..N_ALL_IDS {
            let g = self.glut_now(id);
            if g > 0.01 {
                s.push_str(&format!(" {}:{:.3}", id, g));
            }
        }
        s.push_str("\nG ");
        s.extend(self.great.iter().map(|d| if *d { '1' } else { '0' }));
        s.push_str(&format!("\nP {} {} {} {}\n", self.cycle, self.stones, self.stones_now, self.great_done as u8));
        let r = self.rep;
        s.push_str(&format!("Q {} {} {} {} {} {} {}\n", r[0], r[1], r[2], r[3], self.order_seq, self.skin, self.ach));
        match self.order {
            Some((o, done, deadline)) => s.push_str(&format!(
                "O {} {} {} {} {} {} {} {:.0} {} {:.3}\n",
                o.guild, o.item.mask, o.item.purity, o.item.form, o.need, o.reward, o.rep, o.secs, done, deadline
            )),
            None => s.push_str("O -\n"),
        }
        s
    }

    /// Принимает строку из econ_save. false - строка не про экономику.
    pub fn econ_line(&mut self, line: &str) -> bool {
        let (tag, rest) = match line.split_once(' ') {
            Some(p) => p,
            None => (line, ""),
        };
        let nums: Vec<f64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        match tag {
            "S" => {
                self.glut.iter_mut().for_each(|g| *g = 0.0);
                for p in rest.split_whitespace() {
                    if let Some((id, g)) = p.split_once(':') {
                        if let (Ok(id), Ok(g)) = (id.parse::<usize>(), g.parse::<f32>()) {
                            if id < N_ALL_IDS && g.is_finite() && g > 0.0 {
                                self.glut[id] = g.min(1e9);
                                self.glut_t[id] = self.time;
                            }
                        }
                    }
                }
            }
            "G" => {
                for (i, ch) in rest.chars().take(N_ITEM_IDS).enumerate() {
                    self.great[i] = ch == '1';
                }
            }
            "P" if nums.len() >= 3 => {
                self.cycle = nums[0] as u32;
                self.stones = nums[1] as u32;
                self.stones_now = nums[2] as u32;
                self.great_done = nums.get(3).map_or(false, |v| *v != 0.0);
            }
            "Q" if nums.len() >= 6 => {
                for g in 0..4 {
                    self.rep[g] = nums[g] as u32;
                }
                let seq = nums[4] as u32;
                if seq != self.order_seq || self.offers.len() < 3 {
                    self.order_seq = seq;
                    self.gen_offers();
                }
                self.skin = (nums[5] as u8).min(4);
                self.ach = nums.get(6).map_or(self.ach, |v| *v as u32);
            }
            "O" => {
                self.order = if nums.len() >= 10 && nums[0] < 4.0 && (1.0..16.0).contains(&nums[1]) {
                    let item = Item::new(nums[1] as u8, (nums[2] as u8).clamp(1, 3), (nums[3] as u8).min(FORM_STONE));
                    let o = Order { guild: nums[0] as u8, item, need: nums[4] as u32, reward: nums[5], rep: nums[6] as u32, secs: nums[7] };
                    Some((o, nums[8] as u32, nums[9]))
                } else {
                    None
                };
            }
            _ => return false,
        }
        true
    }

    // ---------- слой сигналов ----------

    pub fn node(&self, x: i32, y: i32) -> Option<&Node> {
        if Self::inb(x, y) {
            self.logic[(y * W + x) as usize].as_ref()
        } else {
            None
        }
    }

    fn set_node(&mut self, x: i32, y: i32, kind: Kind, lk: Lk, dir: Dir) {
        let slot = &mut self.logic[(y * W + x) as usize];
        match slot {
            Some(n) if n.kind == kind => n.dir = dir,
            _ => {
                if slot.is_none() {
                    self.n_logic += 1;
                }
                *slot = Some(Node { kind, dir, param: default_lparam(lk), out: false, count: 0, prev: false });
            }
        }
    }

    fn place_logic(&mut self, kind: Kind, lk: Lk, x: i32, y: i32, dir: Dir) -> Result<bool, &'static str> {
        if !Self::inb(x, y) {
            return Err("ЗА ГРАНИЦЕЙ КАРТЫ");
        }
        let old = self.logic[(y * W + x) as usize];
        if let Some(n) = old {
            if n.kind == kind && (n.dir == dir || lk == Lk::Wire) {
                return Ok(false);
            }
        }
        let same = old.map_or(false, |n| n.kind == kind);
        let refund = old.filter(|_| !same).map_or(0.0, |n| self.cost(n.kind));
        let cost = if same { 0.0 } else { self.cost(kind) };
        if self.money + refund < cost {
            return Err(ERR_MONEY);
        }
        self.money += refund - cost;
        self.set_node(x, y, kind, lk, dir);
        Ok(true)
    }

    /// Снимает деталь логики с тайла и возвращает её цену.
    pub fn remove_logic(&mut self, x: i32, y: i32) -> bool {
        match self.node(x, y).map(|n| n.kind) {
            Some(k) => {
                self.money += self.cost(k);
                self.force_clear_logic(x, y);
                true
            }
            None => false,
        }
    }

    pub fn force_clear_logic(&mut self, x: i32, y: i32) {
        if Self::inb(x, y) {
            let i = (y * W + x) as usize;
            if self.logic[i].take().is_some() {
                self.n_logic -= 1;
                self.powered[i] = false;
                self.halted[i] = false;
                self.sw_sel[i] = 0;
            }
        }
    }

    /// Задаёт настройку детали логики. false - детали нет или настраивать нечего.
    pub fn set_lparam(&mut self, x: i32, y: i32, v: u64) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        match self.logic[(y * W + x) as usize].as_mut() {
            Some(n) if lk_has_param(n.lk()) => {
                n.param = clean_lparam(n.lk(), v);
                true
            }
            _ => false,
        }
    }

    /// Включён ли сигнал, приходящий к тайлу `i` со стороны `side`: провод под напряжением
    /// или деталь, чей включённый выход смотрит прямо сюда. Второе значение - есть ли там что-то вообще.
    fn sig_in(&self, i: usize, side: Dir) -> (bool, bool) {
        let (x, y) = (i as i32 % W + side.dx(), i as i32 / W + side.dy());
        if !Self::inb(x, y) {
            return (false, false);
        }
        let j = (y * W + x) as usize;
        match &self.logic[j] {
            Some(n) if n.lk() == Lk::Wire => (self.powered[j], true),
            Some(n) if n.lk() != Lk::Act && n.dir == side.opp() => (n.out, true),
            _ => (false, false),
        }
    }

    /// Один такт логики. Сначала по выходам прошлого такта считается, какие провода под
    /// напряжением, затем все детали разом получают новые выходы - поэтому порядок обхода
    /// тайлов на результат не влияет, а каждый вентиль даёт задержку ровно в один такт.
    fn logic_tick(&mut self) {
        self.logic_ticks += 1;
        if self.n_logic == 0 {
            return;
        }
        let n = self.logic.len();
        self.powered.iter_mut().for_each(|p| *p = false);
        let mut stack: Vec<usize> = Vec::new();
        for i in 0..n {
            if let Some(nd) = &self.logic[i] {
                if nd.out && nd.lk() != Lk::Wire && nd.lk() != Lk::Act {
                    let (x, y) = (i as i32 % W + nd.dir.dx(), i as i32 / W + nd.dir.dy());
                    if Self::inb(x, y) {
                        let j = (y * W + x) as usize;
                        if self.logic[j].map_or(false, |w| w.lk() == Lk::Wire) && !self.powered[j] {
                            self.powered[j] = true;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        while let Some(i) = stack.pop() {
            for d in [Dir::N, Dir::E, Dir::S, Dir::W] {
                let (x, y) = (i as i32 % W + d.dx(), i as i32 / W + d.dy());
                if Self::inb(x, y) {
                    let j = (y * W + x) as usize;
                    if !self.powered[j] && self.logic[j].map_or(false, |w| w.lk() == Lk::Wire) {
                        self.powered[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let cap = self.storage_cap();
        let mut next: Vec<(usize, bool, u32, bool)> = Vec::new();
        for i in 0..n {
            let nd = match self.logic[i] {
                Some(nd) if nd.lk() != Lk::Wire => nd,
                _ => continue,
            };
            let lk = nd.lk();
            // Входы: сзади и с боков; у исполнителя и датчиков направление не важно.
            let back = self.sig_in(i, nd.dir.opp());
            let (l, r) = (self.sig_in(i, nd.dir.ccw()), self.sig_in(i, nd.dir.cw()));
            let side = l.0 || r.0;
            let ins = [back, l, r];
            let any = ins.iter().any(|s| s.0);
            let (mut out, mut count, mut prev) = (nd.out, nd.count, nd.prev);
            let b = self.tiles[i].as_ref();
            match lk {
                // Копия мира в сети своим лентам не верит: показания датчиков приходят от хоста.
                Lk::SenJam | Lk::SenStore if self.replica => {}
                Lk::SenJam => {
                    out = b.map_or(false, |b| {
                        if b.kind.is_track() {
                            b.lanes.iter().any(|ln| ln.len() >= 2 && ln[0].p >= 0.999)
                        } else {
                            b.out.is_some() && b.working == false && !matches!(b.kind.role(), Role::Storage)
                        }
                    })
                }
                Lk::SenStore => {
                    out = b.map_or(false, |b| b.kind.role() == Role::Storage && b.count as u64 * 100 >= nd.param * cap as u64)
                }
                Lk::SenItem => {
                    // Импульс длиной в такт на каждый предмет из очереди, с паузой в такт между ними.
                    if out {
                        out = false;
                    } else if count > 0 {
                        count -= 1;
                        out = true;
                    }
                }
                Lk::Timer => {
                    let period = (nd.param.clamp(2, 36_000) * LOGIC_HZ as u64 / 10) as u32;
                    out = count % period < period / 2;
                    count = (count + 1) % period;
                }
                Lk::And => out = any && ins.iter().all(|s| s.0 || !s.1),
                Lk::Or => out = any,
                Lk::Not => out = !any,
                Lk::Latch => {
                    if side {
                        out = false;
                    } else if back.0 {
                        out = true;
                    }
                }
                Lk::Counter => {
                    out = false;
                    if side {
                        count = 0;
                    } else if back.0 && !prev {
                        count += 1;
                        if count as u64 >= nd.param.max(1) {
                            count = 0;
                            out = true;
                        }
                    }
                    prev = back.0;
                }
                Lk::Act => {
                    let on = any || self.sig_in(i, nd.dir).0;
                    out = on;
                }
                Lk::Wire => {}
            }
            if matches!(lk, Lk::SenJam | Lk::SenStore) && out != nd.out {
                self.logic_dirty = true;
            }
            next.push((i, out, count, prev));
        }
        for (i, out, count, prev) in next {
            let nd = self.logic[i].as_mut().unwrap();
            nd.out = out;
            nd.count = count;
            nd.prev = prev;
            if nd.lk() == Lk::Act {
                // Переключателю сигнал выбирает выход, всё остальное - останавливает или запускает.
                let is_switch = self.tiles[i].as_ref().map_or(false, |b| b.kind.role() == Role::Switch);
                let stop = if nd.param & 1 == 0 { out } else { !out };
                self.halted[i] = stop && !is_switch;
                if stop && self.tiles[i].is_some() {
                    self.ach_logic = true;
                }
                self.sw_sel[i] = if is_switch { 1 + out as u8 } else { 0 };
            }
        }
    }

    /// Остановлена ли постройка на тайле исполнителем.
    pub fn is_halted(&self, x: i32, y: i32) -> bool {
        Self::inb(x, y) && self.halted[(y * W + x) as usize]
    }

    /// Состояние всех деталей логики по порядку тайлов - хост рассылает его клиентам.
    /// Провода своего состояния не имеют (напряжение считается заново каждый такт) и в счёт не идут.
    pub fn logic_state(&self) -> Vec<(bool, u32, bool)> {
        self.logic.iter().flatten().filter(|n| n.lk() != Lk::Wire).map(|n| (n.out, n.count, n.prev)).collect()
    }

    /// Клиент принимает состояние логики хоста (если набор деталей совпадает).
    pub fn apply_logic_state(&mut self, ticks: u64, st: &[(bool, u32, bool)]) {
        if st.len() != self.logic.iter().flatten().filter(|n| n.lk() != Lk::Wire).count() {
            return;
        }
        self.logic_ticks = ticks;
        for (n, s) in self.logic.iter_mut().flatten().filter(|n| n.lk() != Lk::Wire).zip(st) {
            n.out = s.0;
            n.count = s.1;
            n.prev = s.2;
        }
    }

    // ---------- каталог и статистика ----------

    /// Продавался ли вид предмета (смесь и форма) хоть в какой-то чистоте.
    pub fn kind_known(&self, kind_idx: usize) -> bool {
        let (mask, form) = (kind_idx % 15, kind_idx / 15);
        (0..3).any(|p| self.discovered.get(mask + 15 * p + 45 * form).copied().unwrap_or(false))
    }

    /// Сколько позиций сетки каталога (без философского камня) уже открыто.
    pub fn catalog_found(&self) -> usize {
        self.discovered[..N_ITEM_IDS].iter().filter(|d| **d).count()
    }

    /// Чего не хватает до полного каталога, от самого дешёвого к самому дорогому.
    pub fn catalog_missing(&self) -> Vec<Item> {
        let mut v: Vec<Item> = (0..N_ITEM_IDS).filter(|i| !self.discovered[*i]).map(Item::from_id).collect();
        v.sort_by(|a, b| a.value().total_cmp(&b.value()).then(a.id().cmp(&b.id())));
        v
    }

    /// Переходит к ведру текущей минуты, обнуляя те, что вышли из окна статистики.
    fn stat_roll(&mut self) {
        let m = (self.time / 60.0) as i64;
        let d = m - self.stat_min;
        if d == 0 {
            return;
        }
        self.stat_min = m;
        if d < 0 || d >= STAT_MINUTES as i64 {
            self.stat.iter_mut().for_each(|b| b.iter_mut().for_each(|c| *c = 0));
            return;
        }
        for _ in 0..d {
            self.stat_cur = (self.stat_cur + 1) % STAT_MINUTES;
            self.stat[self.stat_cur].iter_mut().for_each(|c| *c = 0);
        }
    }

    /// Продажи в лавке за окно статистики: (номер позиции, штук), от самых частых.
    pub fn recent_sales(&self) -> Vec<(usize, u32)> {
        let mut v: Vec<(usize, u32)> = (0..N_ALL_IDS)
            .map(|i| (i, self.stat.iter().map(|b| b[i]).sum::<u32>()))
            .filter(|(_, n)| *n > 0)
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Сколько секунд на самом деле покрывает окно статистики (в начале игры - меньше полного).
    pub fn stat_window(&self) -> f64 {
        let full = (STAT_MINUTES as f64 - 1.0) * 60.0 + self.time % 60.0;
        full.min(self.time).max(1.0)
    }

    // ---------- алтари ----------

    /// Все алтари, на которых сейчас горит изделие, с их итоговыми радиусами.
    /// Радиус: базовый + эфир самого алтаря или сильнейший эфир соседнего алтаря,
    /// который достаёт до этого своим собственным радиусом (цепочки не складываются).
    pub fn altars(&self) -> Vec<AltarInfo> {
        let mut v: Vec<AltarInfo> = Vec::new();
        for (i, t) in self.tiles.iter().enumerate() {
            if let Some(b) = t {
                if b.kind.role() == Role::Altar {
                    if let Some(it) = b.slots[0] {
                        let s = aura_of(it);
                        v.push(AltarInfo { x: i as i32 % W, y: i as i32 / W, s, r: ALTAR_RADIUS + aura_radius_bonus(s[3]) });
                    }
                }
            }
        }
        let own: Vec<(i32, i32, f32, i32)> = v.iter().map(|a| (a.x, a.y, a.s[3], a.r)).collect();
        for a in &mut v {
            for &(x, y, s, r) in &own {
                if (x, y) != (a.x, a.y) && (x - a.x).abs().max((y - a.y).abs()) <= r {
                    a.r = a.r.max(ALTAR_RADIUS + aura_radius_bonus(s));
                }
            }
        }
        v
    }

    /// Ауры на тайле: [скорость, экономия сырья, двойной выход] (сила, а не шанс).
    pub fn aura_at(&self, x: i32, y: i32) -> [f32; 3] {
        if Self::inb(x, y) && self.aura_on {
            self.aura[(y * W + x) as usize]
        } else {
            [0.0; 3]
        }
    }

    /// Сколько приёмников слушает код телепорта.
    pub fn tele_receivers(&self, code: usize) -> usize {
        self.tiles.iter().flatten().filter(|b| b.kind.role() == Role::TeleOut && tele_code(b.cfg) == Some(code)).count()
    }

    /// Пересчитывает ауры и список приёмников телепорта после изменения построек,
    /// настроек или смены изделия на алтаре.
    fn rebuild(&mut self) {
        self.dirty = false;
        self.tele_rx.clear();
        self.tele_tx.clear();
        for (i, t) in self.tiles.iter().enumerate() {
            if let Some(b) = t {
                if let Some(code) = tele_code(b.cfg) {
                    match b.kind.role() {
                        Role::TeleOut => self.tele_rx.push((code as u16, i)),
                        Role::TeleIn => self.tele_tx.push((code as u16, i)),
                        _ => {}
                    }
                }
            }
        }
        self.tele_rx.sort_unstable();
        self.tele_tx.sort_unstable();
        let altars = self.altars();
        if self.aura_on || !altars.is_empty() {
            self.aura.iter_mut().for_each(|a| *a = [0.0; 3]);
        }
        self.aura_on = !altars.is_empty();
        for a in &altars {
            for y in (a.y - a.r).max(0)..=(a.y + a.r).min(H - 1) {
                for x in (a.x - a.r).max(0)..=(a.x + a.r).min(W - 1) {
                    let t = &mut self.aura[(y * W + x) as usize];
                    for k in 0..3 {
                        t[k] = t[k].max(a.s[k]);
                    }
                }
            }
        }
    }

    /// Клиент подводит часы под хоста: от времени мира зависит фаза переключателей.
    pub fn sync_time(&mut self, t: f64) {
        if t.is_finite() && t >= 0.0 {
            self.time = t;
        }
    }

    // ---------- симуляция ----------

    pub fn update(&mut self, dt: f32) {
        // Шаг дробится так, чтобы даже экспресс-лента не проскакивала больше 0.2 тайла.
        let n = ((self.belt_speed() * 2.0 * dt / 0.2).ceil() as i32).clamp(1, 24);
        self.stat_roll();
        self.guild_update();
        // Логика идёт своими тактами по 1/60 с, сколько бы кадров ни рисовалось.
        self.logic_acc += dt;
        while self.logic_acc >= 1.0 / LOGIC_HZ - 1e-6 {
            self.logic_acc -= 1.0 / LOGIC_HZ;
            self.logic_tick();
        }
        if self.ach_logic {
            self.ach_logic = false;
            self.award(10);
        }
        for _ in 0..n {
            self.step(dt / n as f32);
        }
    }

    /// Продаёт одну штуку (ящики лавка разбирает сама и продаёт поштучно).
    fn sell(&mut self, item: Item, mult: f64, at_hub: bool) {
        let id = item.id().min(N_ALL_IDS - 1);
        if at_hub {
            // Статистику ведёт и копия мира в сети: это её собственная оценка, экономику она не трогает.
            self.stat[self.stat_cur][id] += 1;
        }
        if self.replica {
            return;
        }
        // Насыщение рынка: чем больше этой позиции продано недавно, тем ниже цена.
        let glut = self.glut_now(id);
        let v = item.value() * self.price_mult() * mult * market_curve(glut);
        self.glut[id] = (glut + 1.0) as f32;
        self.glut_t[id] = self.time;
        self.money += v;
        self.pending_sold += v;
        self.total_sold += 1;
        // Каталог различает чистоту: первая продажа I, II и III премируется отдельно.
        if !self.discovered[id] {
            self.discovered[id] = true;
            let bonus = item.first_sale_bonus();
            self.money += bonus;
            self.pending_new.push((item, bonus));
        }
        if !at_hub {
            return;
        }
        if let Some(n) = self.sold_by_id.get_mut(item.id()) {
            *n += 1;
        }
        // Великое Делание: по одной штуке каждой позиции каталога.
        if id < N_ITEM_IDS && !self.great[id] {
            self.great[id] = true;
            if !self.great_done && self.great.iter().all(|g| *g) {
                self.great_done = true;
                self.money += GREAT_REWARD;
                self.pending_msgs.push("ВЕЛИКОЕ ДЕЛАНИЕ ЗАВЕРШЕНО! НАГРАДА $5.00B".to_string());
                self.award(6);
            }
        }
        if item.form == FORM_STONE {
            self.stones_now += 1;
            self.award(2);
        }
        // Заказ гильдии.
        if let Some((o, done, deadline)) = self.order {
            if o.item == item {
                if done + 1 >= o.need {
                    self.order = None;
                    self.money += o.reward * self.reward_mult();
                    self.pending_msgs.push(format!("{}: ЗАКАЗ ВЫПОЛНЕН, +${:.0}, РЕПУТАЦИЯ +{}", GUILD_NAMES[o.guild as usize], o.reward * self.reward_mult(), o.rep));
                    self.add_rep(o.guild as usize, o.rep);
                    self.award(7);
                } else {
                    self.order = Some((o, done + 1, deadline));
                }
            }
        }
        let found = self.catalog_found();
        for (i, n) in [(3, 100), (4, N_ITEM_IDS / 2), (5, N_ITEM_IDS)] {
            if found >= n {
                self.award(i);
            }
        }
        let c = contract(self.level);
        if item == c.item {
            self.delivered += 1;
            if self.delivered >= c.need {
                self.money += c.reward * self.reward_mult();
                self.award(0);
                if self.level + 1 == FINAL_LEVEL {
                    self.award(1);
                }
                self.level += 1;
                self.delivered = 0;
                self.pending_levelups += 1;
            }
        }
    }

    /// Пытается передать предмет, движущийся в направлении `d`, на тайл (x, y).
    fn try_insert(&mut self, x: i32, y: i32, item: Item, d: Dir, p0: f32) -> bool {
        let ok = self.try_insert_raw(x, y, item, d, p0);
        if ok && self.n_logic > 0 && !self.replica {
            // Датчик предмета над этой постройкой запоминает въехавший предмет.
            // У копии мира в сети предметы на лентах свои, поэтому счёт ей присылает хост.
            if let Some(n) = self.logic[(y * W + x) as usize].as_mut() {
                if n.lk() == Lk::SenItem && Cond::from_bits(n.param).matches(item) {
                    n.count += 1;
                    self.logic_dirty = true;
                }
            }
        }
        ok
    }

    fn try_insert_raw(&mut self, x: i32, y: i32, item: Item, d: Dir, p0: f32) -> bool {
        if !Self::inb(x, y) {
            return false;
        }
        let idx = (y * W + x) as usize;
        let role = match &self.tiles[idx] {
            Some(b) => b.kind.role(),
            None => return false,
        };
        match role {
            // Ящик продаётся как CRATE_SIZE отдельных штук: каталог, контракт и счётчики - по штукам.
            Role::Hub => {
                for _ in 0..item.pieces() {
                    self.sell(item.packed(false), 1.0, true);
                }
                return true;
            }
            Role::GuildPost => {
                for _ in 0..item.pieces() {
                    self.sell(item.packed(false), 1.0, true);
                }
                return true;
            }
            Role::Trade => {
                let m = self.trade_mult();
                for _ in 0..item.pieces() {
                    self.sell(item.packed(false), m, false);
                }
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
        if role == Role::Altar {
            // Алтарь берёт изделия с любой стороны: одно горит, одно ждёт очереди.
            let b = self.tiles[idx].as_mut().unwrap();
            if item.form == 0 || item.boxed {
                return false;
            }
            return match free_slot(&b.slots) {
                Some(0) => {
                    b.slots[0] = Some(item);
                    b.progress = 0.0;
                    self.dirty = true;
                    true
                }
                Some(f) => {
                    b.slots[f] = Some(item);
                    true
                }
                None => false,
            };
        }
        if role == Role::TeleIn {
            // Передатчик берёт с любой стороны всё подряд: предмет сразу улетает в нору.
            let tick = self.tick;
            let b = self.tiles[idx].as_mut().unwrap();
            if b.lanes[0].len() >= TELE_QUEUE {
                return false;
            }
            b.lanes[0].push(BeltItem { item, p: 0.0, d, tick });
            return true;
        }
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
            Role::Splitter | Role::Overflow | Role::Filter | Role::Switch | Role::Ratio | Role::Sorter => {
                if closed || b.slots[0].is_some() {
                    return false;
                }
                b.slots[0] = Some(item);
                true
            }
            Role::Mixer | Role::Distiller | Role::Assemble(..) | Role::Crucible | Role::Transmuter => {
                if closed {
                    return false;
                }
                let free = free_slot(&b.slots);
                // То, что уже лежит внутри: с ним новый предмет должен сочетаться.
                let other = match free {
                    Some(f) => b.slots[1 - f],
                    None => b.slots[0],
                };
                let ok = !item.boxed
                    && match role {
                        Role::Mixer => item.form == 0 && other.map_or(true, |o| o.mask & item.mask == 0),
                        Role::Distiller => item.form == 0 && item.purity < 3 && other.map_or(true, |o| o == item),
                        Role::Assemble(fa, fb, _, _) => {
                            (item.form == fa || item.form == fb)
                                && other.map_or(true, |o| o.mask == item.mask && o.form != item.form)
                        }
                        // Квинтэссенцию сливать уже не с чем; камень в тигель не идёт.
                        Role::Crucible => {
                            item.form >= 1
                                && item.form != FORM_STONE
                                && item.mask != 15
                                && other.map_or(true, |o| o.form == item.form && o.mask & item.mask == 0)
                        }
                        Role::Transmuter => {
                            item.form == 0 && item.mask.count_ones() == 1 && other.map_or(true, |o| o.mask == item.mask)
                        }
                        _ => false,
                    };
                match free {
                    Some(f) if ok => {
                        b.slots[f] = Some(item);
                        true
                    }
                    // Подходящему предмету просто нет места - он ждёт. Неподходящий уходит в отказ.
                    None if ok => false,
                    _ => Self::take_reject(b, item),
                }
            }
            Role::Packer => {
                if closed {
                    return false;
                }
                if !item.boxed && (b.count == 0 || b.slots[0] == Some(item)) {
                    if b.count >= CRATE_SIZE {
                        return false;
                    }
                    b.slots[0] = Some(item);
                    b.count += 1;
                    true
                } else {
                    // Другой предмет при начатом ящике (или готовый ящик) - в отказ.
                    Self::take_reject(b, item)
                }
            }
            Role::Unpacker => {
                if closed {
                    return false;
                }
                if !item.boxed {
                    return Self::take_reject(b, item);
                }
                if b.count > 0 {
                    return false;
                }
                b.slots[0] = Some(item.packed(false));
                b.count = CRATE_SIZE;
                true
            }
            Role::Opus => {
                if closed {
                    return false;
                }
                // У каждой чистой стихии своя ячейка, поэтому магистерии можно везти одной лентой.
                let pure = item.form == FORM_MAGISTERIUM && !item.boxed && item.mask.count_ones() == 1;
                let e = item.mask.trailing_zeros() as usize;
                if pure && b.bins[e] < OPUS_CELL {
                    b.binp[e] = if b.bins[e] == 0 { item.purity } else { b.binp[e].min(item.purity) };
                    b.bins[e] += 1;
                    true
                } else {
                    // Чужой предмет или лишний магистерий в полную ячейку - в отказ, если он назначен.
                    Self::take_reject(b, item)
                }
            }
            Role::Convert(from, _, _) => {
                if closed || item.boxed || item.form != from || b.slots[0].is_some() {
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
            Role::Extractor(_) | Role::Hub | Role::Trade | Role::Trash | Role::Altar | Role::TeleIn | Role::TeleOut => false,
            Role::Logic(_) | Role::GuildPost => false,
        }
    }

    /// Кладёт неподходящий предмет в ячейку отказа, если у машины назначена сторона отказа.
    fn take_reject(b: &mut Building, item: Item) -> bool {
        if reject_rel(b).is_none() || b.rej.is_some() {
            return false;
        }
        b.rej = Some(item);
        true
    }

    /// Выдаёт отказанный предмет через сторону отказа.
    fn push_rej(&mut self, i: usize, x: i32, y: i32) {
        let (it, od) = match self.tiles[i].as_ref() {
            Some(b) => match (b.rej, reject_rel(b)) {
                (Some(it), Some(r)) => (it, Dir::from_u8(b.dir as u8 + r as u8)),
                _ => return,
            },
            None => return,
        };
        if self.try_insert(x + od.dx(), y + od.dy(), it, od, 0.0) {
            self.tiles[i].as_mut().unwrap().rej = None;
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
                // Двойной выход: тот же предмет выдаётся ещё раз.
                if b.extra > 0 {
                    b.extra -= 1;
                } else {
                    b.out = None;
                }
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
        if self.dirty {
            self.rebuild();
        }
        for i in 0..self.tiles.len() {
            let (role, dir) = match &self.tiles[i] {
                Some(b) => (b.kind.role(), b.dir),
                None => continue,
            };
            if self.n_logic > 0 && self.halted[i] {
                // Исполнитель остановил постройку: лента стоит воротами, машина не работает и не выдаёт.
                continue;
            }
            let x = i as i32 % W;
            let y = i as i32 / W;
            // Ауры алтарей на этом тайле: скорость, экономия сырья, двойной выход.
            let au = if self.aura_on { self.aura[i] } else { [0.0; 3] };
            let (fast, keep_p, twice_p) = (1.0 + au[0], aura_chance(au[1]), aura_chance(au[2]));
            match role {
                Role::Hub | Role::Trash | Role::Trade | Role::Logic(_) | Role::GuildPost => {}
                Role::Altar => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = b.slots[0].is_some();
                    if b.working {
                        b.progress += dt / ALTAR_BURN;
                        if b.progress >= 1.0 {
                            // Изделие сгорело: на его место встаёт следующее из очереди.
                            b.slots[0] = b.slots[1].take();
                            b.progress = if b.slots[0].is_some() { b.progress - 1.0 } else { 0.0 };
                            self.dirty = true;
                        }
                    }
                }
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
                    let rich = self.rich[i];
                    if rich && self.tiles[i].as_ref().map_or(false, |b| b.out.is_some()) {
                        self.award(11);
                    }
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if v > 0 && b.out.is_none() {
                        b.working = true;
                        b.progress += er * m * level_mult(b.level) * fast;
                        if b.progress >= 1.0 {
                            b.progress = 0.0;
                            b.out = Some(Item::new(1 << (v - 1), 1 + rich as u8, 0));
                            b.extra = bump(&mut b.acc[1], twice_p) as u8;
                        }
                    }
                    self.push_out(i, x, y, dir);
                }
                Role::Packer => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = b.count > 0;
                    if b.out.is_none() && b.count >= CRATE_SIZE {
                        b.out = b.slots[0].map(|it| it.packed(true));
                        b.slots[0] = None;
                        b.count = 0;
                    }
                    self.push_out(i, x, y, dir);
                    self.push_rej(i, x, y);
                }
                Role::Unpacker => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = b.count > 0;
                    if b.out.is_none() && b.count > 0 {
                        b.out = b.slots[0];
                        b.count -= 1;
                        if b.count == 0 {
                            b.slots[0] = None;
                        }
                    }
                    self.push_out(i, x, y, dir);
                    self.push_rej(i, x, y);
                }
                Role::TeleOut => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = b.out.is_some();
                    self.push_out(i, x, y, dir);
                }
                Role::TeleIn => {
                    // Все предметы в норе летят одновременно, каждый - TELE_PERIOD секунд.
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = !b.lanes[0].is_empty();
                    for bi in &mut b.lanes[0] {
                        bi.p = (bi.p + dt / TELE_PERIOD).min(1.0);
                    }
                    let code = match tele_code(b.cfg) {
                        Some(c) => c,
                        None => continue,
                    };
                    let lo = self.tele_rx.partition_point(|r| (r.0 as usize) < code);
                    let n = self.tele_rx.partition_point(|r| r.0 as usize <= code) - lo;
                    // Долетевшие выходят у приёмников кода по очереди; если все заняты - ждут в норе.
                    let t0 = self.tele_tx.partition_point(|r| (r.0 as usize) < code);
                    let t1 = self.tele_tx.partition_point(|r| r.0 as usize <= code);
                    'arrive: while n > 0 {
                        let it = match self.tiles[i].as_ref().and_then(|b| b.lanes[0].first()) {
                            Some(bi) if bi.p >= 1.0 => bi.item,
                            _ => break,
                        };
                        // Чья очередь: первый передатчик с долетевшим предметом после отдавшего последним.
                        let last = self.tele_last[code] as usize;
                        let ready = |j: usize| {
                            self.tiles[j].as_ref().and_then(|b| b.lanes[0].first()).map_or(false, |bi| bi.p >= 1.0)
                        };
                        let turn = self.tele_tx[t0..t1]
                            .iter()
                            .map(|r| r.1)
                            .filter(|j| ready(*j))
                            .min_by_key(|j| (*j <= last && last != u32::MAX as usize, *j));
                        if turn != Some(i) {
                            break;
                        }
                        let start = self.tele_rr[code] as usize;
                        for k in 0..n {
                            let r = self.tele_rx[lo + (start + k) % n].1;
                            if self.tiles[r].as_ref().map_or(false, |b| b.out.is_none()) {
                                self.tiles[r].as_mut().unwrap().out = Some(it);
                                self.tiles[i].as_mut().unwrap().lanes[0].remove(0);
                                self.tele_rr[code] = ((start + k + 1) % n) as u16;
                                self.tele_last[code] = i as u32;
                                continue 'arrive;
                            }
                        }
                        break;
                    }
                }
                Role::Opus => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if b.out.is_none() && b.bins.iter().all(|n| *n > 0) {
                        b.working = true;
                        b.progress += ms * level_mult(b.level) * fast / OPUS_TIME;
                        if b.progress >= 1.0 {
                            b.progress = 0.0;
                            let purity = b.binp.iter().copied().min().unwrap_or(1).clamp(1, 3);
                            if !bump(&mut b.acc[0], keep_p) {
                                b.bins.iter_mut().for_each(|n| *n -= 1);
                            }
                            b.out = Some(Item::new(15, purity, FORM_STONE));
                            b.extra = bump(&mut b.acc[1], twice_p) as u8;
                        }
                    }
                    self.push_out(i, x, y, dir);
                    self.push_rej(i, x, y);
                }
                Role::Mixer | Role::Distiller | Role::Assemble(..) | Role::Crucible | Role::Transmuter => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if b.out.is_none() {
                        if let (Some(a), Some(c)) = (b.slots[0], b.slots[1]) {
                            let minp = a.purity.min(c.purity);
                            let (t, res) = match role {
                                Role::Mixer => (1.0, Item::new(a.mask | c.mask, minp, 0)),
                                Role::Distiller => (1.5, Item::new(a.mask, a.purity + 1, 0)),
                                Role::Assemble(_, _, to, t) => (t, Item::new(a.mask, minp, to)),
                                Role::Crucible => (3.0, Item::new(a.mask | c.mask, minp, a.form)),
                                Role::Transmuter => (3.0, Item::new(next_element(a.mask), minp, 0)),
                                _ => (1.0, a),
                            };
                            b.working = true;
                            b.progress += ms * level_mult(b.level) * fast / t;
                            if b.progress >= 1.0 {
                                b.progress = 0.0;
                                // Экономия сырья: входы остаются в машине на следующий цикл.
                                if !bump(&mut b.acc[0], keep_p) {
                                    b.slots = [None, None];
                                }
                                b.out = Some(res);
                                b.extra = bump(&mut b.acc[1], twice_p) as u8;
                            }
                        }
                    }
                    self.push_out(i, x, y, dir);
                    self.push_rej(i, x, y);
                }
                Role::Convert(_, to, t) => {
                    let b = self.tiles[i].as_mut().unwrap();
                    b.working = false;
                    if b.out.is_none() {
                        if let Some(a) = b.slots[0] {
                            b.working = true;
                            b.progress += ms * level_mult(b.level) * fast / t;
                            if b.progress >= 1.0 {
                                b.progress = 0.0;
                                if !bump(&mut b.acc[0], keep_p) {
                                    b.slots[0] = None;
                                }
                                b.out = Some(Item::new(a.mask, a.purity, to));
                                b.extra = bump(&mut b.acc[1], twice_p) as u8;
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
                Role::Splitter | Role::Overflow | Role::Filter | Role::Switch | Role::Ratio | Role::Sorter => {
                    let (it, rr, cfg, all4, cnt, level, phase) = match self.tiles[i].as_ref() {
                        Some(b) => match b.slots[0] {
                            Some(it) => {
                                let (o, n) = out_dirs(b);
                                (it, b.rr, b.cfg, o, n, b.level, b.count)
                            }
                            None => continue,
                        },
                        None => continue,
                    };
                    if cnt == 0 {
                        continue;
                    }
                    let all = &all4[..cnt];
                    // Сортировочный узел: выходы идут в порядке вперёд, вправо, влево - как и условия.
                    let mut pick = [Dir::N; 3];
                    let mut pn = 0;
                    if role == Role::Sorter && cnt == 3 {
                        let conds = [0, 1, 2].map(|k| sorter_cond(cfg, k));
                        match (0..3).find(|k| !conds[*k].is_any() && conds[*k].matches(it)) {
                            // Первый выход, чьё условие подошло.
                            Some(k) => {
                                pick[0] = all[k];
                                pn = 1;
                            }
                            // Иначе - выходы без условия ("всё остальное"), первый свободный.
                            None => {
                                for k in 0..3 {
                                    if conds[k].is_any() {
                                        pick[pn] = all[k];
                                        pn += 1;
                                    }
                                }
                            }
                        }
                    }
                    let (outs, rot): (&[Dir], u8) = match role {
                        Role::Sorter => (&pick[..pn], 0),
                        Role::Splitter => (&all[..], rr),
                        Role::Overflow => (&all[..], 0),
                        Role::Switch => {
                            // Открыт ровно один выход; если он занят, предмет ждёт.
                            // Исполнитель над переключателем выбирает выход сигналом вместо таймера.
                            let k = match self.sw_sel[i] {
                                0 => (self.time / switch_period(level)) as usize % cnt,
                                s => (s as usize - 1).min(cnt - 1),
                            };
                            (&all[k..k + 1], 0)
                        }
                        Role::Ratio => {
                            // Сначала N штук в первый выход, потом M во второй; занятый выход ждут.
                            let (n, _) = ratio_nm(cfg);
                            let k = (cnt > 1 && phase >= n) as usize;
                            (&all[k..k + 1], 0)
                        }
                        _ => {
                            let mut cond = Cond::from_bits(cfg);
                            if cond.is_any() && !self.replica {
                                // Образец не задан: им становится первый проехавший предмет.
                                cond = Cond::exact(it);
                                self.tiles[i].as_mut().unwrap().cfg = cond.bits();
                                self.pending_cfg.push((x, y, cond.bits()));
                            }
                            if cond.matches(it) {
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
                            if role == Role::Ratio {
                                let (n, m) = ratio_nm(cfg);
                                b.count = (phase + 1) % (n + m);
                            }
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
                Role::Hub | Role::Trash | Role::Trade | Role::Altar | Role::TeleIn | Role::GuildPost => false,
                Role::Bridge => true,
                Role::Belt(_) => n.dir == side.opp(),
                // Вход пары уводит предметы под землю, наружу выдаёт только выход или одиночка.
                Role::Tunnel => {
                    n.dir == side.opp() && self.tunnel_exit(x + side.dx(), y + side.dy()).is_none()
                }
                _ => {
                    let rel = rel_side(n.dir, side.opp());
                    side_out(n, rel) || reject_rel(n) == Some(rel)
                }
            },
            None => false,
        }
    }

    /// Принимает ли тайл (x, y) предметы, едущие в направлении `d`? Тоже для отрисовки стыков.
    pub fn accepts_from(&self, x: i32, y: i32, d: Dir) -> bool {
        match self.get(x, y) {
            Some(n) => match n.kind.role() {
                Role::Hub | Role::Trash | Role::Trade | Role::Bridge | Role::Altar | Role::TeleIn | Role::GuildPost => true,
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
        // Строка D - каталог в старом виде (390 видов без чистоты): её понимают прежние версии.
        s.push_str("D ");
        s.extend((0..N_KINDS).map(|i| if self.kind_known(i) { '1' } else { '0' }));
        s.push('\n');
        s.push_str(&self.econ_save());
        // Карта новой версии: разломы и преграды строятся из сида, в файле - только снесённое.
        if self.map_ver >= 2 {
            s.push_str("M 2\n");
        }
        for i in &self.cleared {
            s.push_str(&format!("X {} {}\n", *i as i32 % W, *i as i32 / W));
        }
        for (i, r) in self.rich.iter().enumerate() {
            if *r {
                s.push_str(&format!("R {} {}\n", i as i32 % W, i as i32 / W));
            }
        }
        // Слой сигналов: "L x y вид направление настройка".
        for (i, n) in self.logic.iter().enumerate() {
            if let Some(n) = n {
                s.push_str(&format!("L {} {} {} {} {}\n", i as i32 % W, i as i32 / W, n.kind.0, n.dir as u8, n.param));
            }
        }
        // Память деталей (защёлка, счётчик, фаза таймера): "LS x:y:выход:счёт:фронт" - одним
        // словом, чтобы прежние версии не приняли строку за постройку.
        for (i, n) in self.logic.iter().enumerate() {
            if let Some(n) = n.filter(|n| n.out || n.count != 0 || n.prev) {
                s.push_str(&format!("LS {}:{}:{}:{}:{}\n", i as i32 % W, i as i32 / W, n.out as u8, n.count, n.prev as u8));
            }
        }
        // Строка K - каталог по позициям с чистотой, по Item::id().
        s.push_str("K ");
        s.extend(self.discovered.iter().map(|d| if *d { '1' } else { '0' }));
        s.push('\n');
        s.push_str(&format!("U {} {} {} {}\n", u[4], u[5], u[6], u[7]));
        // Время мира: от него зависит фаза переключателей.
        s.push_str(&format!("T {:.3}\n", self.time));
        for (i, t) in self.tiles.iter().enumerate() {
            if let Some(b) = t {
                if b.kind != Kind::Hub {
                    s.push_str(&format!("{} {} {} {} {}", i as i32 % W, i as i32 / W, b.kind.0, b.dir as u8, b.level));
                    // Настройка сторон пишется шестым числом, только если она не по умолчанию.
                    if b.ports != [PORT_AUTO; 4] || b.cfg != 0 {
                        s.push_str(&format!(" {}", pack_ports(b.ports)));
                    }
                    // Настройка постройки - седьмым числом, тоже только если задана.
                    if b.cfg != 0 {
                        s.push_str(&format!(" {}", b.cfg));
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
        // Карта: сначала разломы и преграды (если мир создан с ними), потом снесённое и дары гильдий.
        if text.lines().any(|l| l.trim() == "M 2") {
            w.map_ver = 2;
            w.gen_features();
        }
        for l in text.lines() {
            let xy = |p: &str| -> Option<usize> {
                let v: Vec<i32> = l.strip_prefix(p)?.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                (v.len() == 2 && Self::inb(v[0], v[1])).then(|| (v[1] * W + v[0]) as usize)
            };
            if let Some(i) = xy("X ") {
                w.rocks[i] = false;
                w.cleared.push(i);
            }
            if let Some(i) = xy("R ") {
                w.rich[i] = w.veins[i] > 0;
            }
        }
        let (mut old_catalog, mut catalog) = (None, None);
        let mut lstate: Vec<Vec<u32>> = Vec::new();
        for line in lines {
            if let Some(l) = line.strip_prefix("LS ") {
                lstate.push(l.trim().split(':').filter_map(|v| v.parse().ok()).collect());
                continue;
            }
            if line.starts_with("X ") || line.starts_with("R ") || line.starts_with("M ") {
                continue;
            }
            if matches!(line.as_bytes().first(), Some(b'S' | b'G' | b'P' | b'Q' | b'O')) && w.econ_line(line) {
                continue;
            }
            if let Some(d) = line.strip_prefix("D ") {
                old_catalog = Some(d);
                continue;
            }
            if let Some(k) = line.strip_prefix("K ") {
                catalog = Some(k);
                continue;
            }
            if let Some(u) = line.strip_prefix("U ") {
                // Новые улучшения хранятся отдельной строкой: старые сохранения её просто не содержат.
                for (i, v) in u.split_whitespace().filter_map(|v| v.parse::<u8>().ok()).take(4).enumerate() {
                    w.upgrades[4 + i] = v.min(UPGRADE_MAX[4 + i]);
                }
                continue;
            }
            if let Some(l) = line.strip_prefix("L ") {
                let v: Vec<u64> = l.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                if v.len() == 5 && Self::inb(v[0] as i32, v[1] as i32) && v[2] < 256 {
                    if let Some(kind) = Kind::from_u8(v[2] as u8) {
                        if let Role::Logic(lk) = kind.role() {
                            w.set_node(v[0] as i32, v[1] as i32, kind, lk, Dir::from_u8(v[3] as u8));
                            w.set_lparam(v[0] as i32, v[1] as i32, v[4]);
                        }
                    }
                }
                continue;
            }
            if let Some(t) = line.strip_prefix("T ") {
                w.time = t.trim().parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(0.0);
                continue;
            }
            let nums: Vec<i64> = line.split_whitespace().filter_map(|v| v.parse().ok()).collect();
            // Пятое число - уровень машины; в старых сохранениях его нет, тогда уровень 1.
            if nums.len() < 4 || nums.len() > 7 {
                continue;
            }
            let v: Vec<i32> = nums.iter().take(6).map(|n| (*n).clamp(-1, 100_000) as i32).collect();
            if !Self::inb(v[0], v[1]) {
                continue;
            }
            if let Some(kind) = Kind::from_u8(v[2] as u8).filter(|k| !matches!(k.role(), Role::Logic(_))) {
                if w.can_place(kind, v[0], v[1]).is_ok() {
                    let mut b = Building::new(kind, Dir::from_u8(v[3] as u8));
                    if Self::can_level(kind) {
                        b.level = (*v.get(4).unwrap_or(&1)).clamp(1, MAX_LEVEL as i32) as u8;
                    }
                    if port_modes(kind) > 0 {
                        b.ports = unpack_ports(*v.get(5).unwrap_or(&0) as u8);
                    }
                    w.tiles[(v[1] * W + v[0]) as usize] = Some(b);
                    if let Some(cfg) = nums.get(6) {
                        w.set_cfg(v[0], v[1], *cfg as u64);
                    }
                }
            }
        }
        match (catalog, old_catalog) {
            (Some(k), _) => {
                for (i, ch) in k.chars().take(N_ALL_IDS).enumerate() {
                    w.discovered[i] = ch == '1';
                }
            }
            // Старое сохранение знает только вид предмета: он засчитывается как чистота I,
            // премии за II и III ещё можно получить.
            (None, Some(d)) => {
                for (i, ch) in d.chars().take(N_KINDS).enumerate() {
                    if ch == '1' {
                        w.discovered[i % 15 + 45 * (i / 15)] = true;
                    }
                }
            }
            (None, None) => {}
        }
        // Память деталей логики читается после самих деталей, где бы её строки ни стояли.
        for v in lstate {
            if v.len() == 5 && Self::inb(v[0] as i32, v[1] as i32) {
                if let Some(n) = w.logic[(v[1] as i32 * W + v[0] as i32) as usize].as_mut().filter(|n| n.lk() != Lk::Wire) {
                    n.out = v[2] == 1;
                    n.count = v[3];
                    n.prev = v[4] == 1;
                }
            }
        }
        // Затоваренность рынка записана на момент сохранения - время мира читается позже неё.
        let now = w.time;
        w.glut_t.iter_mut().for_each(|t| *t = now);
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

    fn run_fps(w: &mut World, secs: f32, fps: u32) {
        for _ in 0..(secs * fps as f32).round() as u32 {
            w.update(1.0 / fps as f32);
        }
    }

    /// Частоты кадров, на которых результат обязан совпадать.
    const FPS: [u32; 3] = [30, 60, 144];

    fn lane_items(w: &World, x: i32, y: i32) -> Vec<Item> {
        w.get(x, y).map_or(Vec::new(), |b| b.lanes[0].iter().map(|bi| bi.item).collect())
    }

    /// Закрытый со всех сторон, кроме входа, склад - счётчик того, что в него приехало.
    fn sink(w: &mut World, x: i32, y: i32, moving: Dir) {
        w.place(Kind::by_name("СКЛАД"), x, y, moving).unwrap();
        w.set_ports(x, y, pack_ports([PORT_OFF, PORT_OFF, PORT_IN, PORT_OFF]));
    }

    fn stored(w: &World, x: i32, y: i32) -> u32 {
        w.get(x, y).map_or(0, |b| b.count + b.out.is_some() as u32)
    }

    /// Образец фильтра задаётся по частям (здесь - только чистота) и пишется в сохранение.
    #[test]
    fn filter_sample_is_chosen_and_saved() {
        let mut w = world();
        let filter = Kind::by_name("ФИЛЬТР");
        w.place(filter, 11, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 12, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        let cond = Cond { purity: 2, ..Cond::default() };
        assert!(w.set_cfg(11, 10, cond.bits()));
        // Чистота II любой смеси и формы идёт прямо, остальное - вбок.
        for it in [Item::new(1, 2, 0), Item::new(6, 1, 4), Item::new(9, 2, 7)] {
            assert!(w.try_insert(11, 10, it, Dir::E, 0.0));
            run(&mut w, 1.0);
        }
        assert_eq!(lane_items(&w, 12, 10), vec![Item::new(1, 2, 0), Item::new(9, 2, 7)]);
        assert_eq!(lane_items(&w, 11, 11), vec![Item::new(6, 1, 4)]);
        assert!(w.pending_cfg.is_empty(), "заданный образец сам не меняется");
        // Сохранение: настройка - седьмое число строки постройки.
        let saved = w.save();
        assert!(saved.contains(&format!("11 10 {} 1 1 0 {}\n", filter.0, cond.bits())), "{}", saved);
        let back = World::load(&saved).unwrap();
        assert_eq!(Cond::from_bits(back.get(11, 10).unwrap().cfg), cond);
        assert_eq!(back.save(), saved);

        // Незаданный образец (как в старых сохранениях) - первый проехавший предмет, и он тоже сохраняется.
        assert!(w.filter_reset(11, 10));
        let steam = Item::new(3, 1, 0);
        assert!(w.try_insert(11, 10, steam, Dir::E, 0.0));
        run(&mut w, 0.5);
        assert_eq!(Cond::from_bits(w.get(11, 10).unwrap().cfg), Cond::exact(steam));
        assert_eq!(w.pending_cfg, vec![(11, 10, Cond::exact(steam).bits())]);
        assert!(Cond::exact(steam).matches(steam) && !Cond::exact(steam).matches(Item::new(3, 2, 0)));
        let back = World::load(&w.save()).unwrap();
        assert_eq!(back.get(11, 10).unwrap().cfg, Cond::exact(steam).bits());
        // Копия мира в сети образец сама не выбирает - ждёт его от хоста.
        let mut rep = World::load(&saved).unwrap();
        rep.replica = true;
        rep.filter_reset(11, 10);
        assert!(rep.try_insert(11, 10, steam, Dir::E, 0.0));
        run(&mut rep, 0.5);
        assert_eq!(rep.get(11, 10).unwrap().cfg, 0);
        // Мусор в настройке не ломает фильтр.
        assert!(w.set_cfg(11, 10, u64::MAX));
        assert!(Cond::from_bits(w.get(11, 10).unwrap().cfg).form as usize <= N_FORMS_ALL);
    }

    /// Дистиллятор с чужой эссенцией в ячейке не запирается: лишнее уходит в сторону отказа.
    #[test]
    fn distiller_reject_side() {
        let mut w = world();
        let dist = Kind::by_name("ДИСТИЛЛЯТОР");
        w.place(dist, 11, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 12, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        let (fire, water) = (Item::base(0), Item::base(1));
        assert!(w.try_insert(11, 10, fire, Dir::E, 0.0));
        // Без стороны отказа - прежнее поведение: чужая эссенция ждёт снаружи.
        assert!(!w.try_insert(11, 10, water, Dir::E, 0.0));
        // Щелчки по правой стороне: авто -> вход -> выход -> закрыто -> отказ.
        let mut seen = Vec::new();
        for _ in 0..4 {
            let (ports, cfg) = w.next_side(11, 10, 1).unwrap();
            w.set_ports(11, 10, ports);
            w.set_cfg(11, 10, cfg);
            seen.push((unpack_ports(ports)[1], cfg));
        }
        assert_eq!(seen, vec![(PORT_IN, 0), (PORT_OUT, 0), (PORT_OFF, 0), (PORT_OFF, 2)]);
        assert_eq!(reject_rel(w.get(11, 10).unwrap()), Some(1));
        let mut w2 = World::load(&w.save()).unwrap();
        assert_eq!(reject_rel(w2.get(11, 10).unwrap()), Some(1), "сторона отказа сохраняется");
        w2.money = 1e9;
        for w in [&mut w, &mut w2] {
            if w.get(11, 10).unwrap().slots == [None, None] {
                assert!(w.try_insert(11, 10, fire, Dir::E, 0.0));
            }
            assert!(w.try_insert(11, 10, water, Dir::E, 0.0), "чужая эссенция принята в отказ");
            assert!(!w.try_insert(11, 10, water, Dir::E, 0.0), "ячейка отказа одна");
            run(w, 1.0);
            assert_eq!(lane_items(w, 11, 11), vec![water]);
            // Изделие дистиллятор тоже не берёт - оно уходит туда же.
            assert!(w.try_insert(11, 10, Item::new(1, 1, 4), Dir::E, 0.0));
            // А своя пара по-прежнему перегоняется и выходит вперёд.
            assert!(w.try_insert(11, 10, fire, Dir::E, 0.0));
            run(w, 4.0);
            assert_eq!(lane_items(w, 12, 10), vec![Item::new(1, 2, 0)]);
            assert_eq!(lane_items(w, 11, 11), vec![water, Item::new(1, 1, 4)]);
        }
        // Ещё щелчок снимает отказ и возвращает сторону в "авто".
        let (ports, cfg) = w.next_side(11, 10, 1).unwrap();
        assert_eq!((unpack_ports(ports)[1], cfg), (PORT_AUTO, 0));
        // Если сторону отказа открыть обычной настройкой сторон, отказ снимается сам.
        assert!(w.set_ports(11, 10, pack_ports([PORT_AUTO, PORT_OUT, PORT_AUTO, PORT_AUTO])));
        assert_eq!(reject_rel(w.get(11, 10).unwrap()), None);
    }

    /// Делитель с пропорцией 3:1 делит ровно 3 к 1 при любой частоте кадров.
    #[test]
    fn ratio_splitter_keeps_proportion() {
        let ratio = Kind::by_name("ДЕЛИТЕЛЬ С ПРОПОРЦИЕЙ");
        let mut totals = Vec::new();
        for fps in FPS {
            let mut w = world();
            w.veins[(10 * W + 10) as usize] = 1;
            w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
            w.place(ratio, 11, 10, Dir::E).unwrap();
            assert!(w.set_cfg(11, 10, ratio_cfg(3, 1)));
            sink(&mut w, 12, 10, Dir::E);
            sink(&mut w, 11, 11, Dir::S);
            run_fps(&mut w, 60.0, fps);
            let (a, b) = (stored(&w, 12, 10), stored(&w, 11, 11));
            assert!(a + b >= 40, "{} к/с: прошло всего {}", fps, a + b);
            assert!(a >= 3 * b && a <= 3 * b + 3, "{} к/с: {} и {} - не 3:1", fps, a, b);
            totals.push(a + b);
            // Пропорция переживает сохранение.
            let back = World::load(&w.save()).unwrap();
            assert_eq!(ratio_nm(back.get(11, 10).unwrap().cfg), (3, 1));
        }
        let (lo, hi) = (totals.iter().min().unwrap(), totals.iter().max().unwrap());
        assert!(hi - lo <= 1, "результат зависит от частоты кадров: {:?}", totals);
        // Незаданная пропорция - 1:1, значения вне 1..9 обрезаются.
        assert_eq!(ratio_nm(0), (1, 1));
        assert_eq!(ratio_nm(clean_cfg(Role::Ratio, 0xFF)), (9, 9));
        // Если второй выход занят, делитель ждёт, а не сливает всё в первый.
        let mut w = world();
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(ratio, 11, 10, Dir::E).unwrap();
        sink(&mut w, 12, 10, Dir::E);
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        run(&mut w, 60.0);
        assert_eq!((stored(&w, 12, 10), lane_items(&w, 11, 11).len()), (3, 2));
    }

    /// Фаза переключателя считается от времени мира: оно пишется в сохранение (снимок для
    /// клиента) и подводится под хоста, поэтому у хоста и клиента открыт один и тот же выход.
    #[test]
    fn switch_phase_follows_host_time() {
        let mut host = world();
        let sw = Kind::by_name("ПЕРЕКЛЮЧАТЕЛЬ");
        host.place(sw, 11, 10, Dir::E).unwrap();
        host.set_ports(11, 10, pack_ports([PORT_OUT, PORT_OUT, PORT_IN, PORT_OFF]));
        host.place(Kind::Belt, 12, 10, Dir::E).unwrap();
        host.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        run(&mut host, 30.0);
        let mut client = World::load(&host.save()).unwrap();
        client.replica = true;
        assert!((client.time - host.time).abs() < 0.002, "{} и {}", client.time, host.time);
        // Хост ушёл вперёд на полпериода, клиент отстал - сообщение о состоянии подводит часы.
        run(&mut host, 15.0);
        run(&mut client, 5.0);
        client.sync_time(host.time);
        for w in [&mut host, &mut client] {
            assert!(w.try_insert(11, 10, Item::base(0), Dir::E, 0.0));
            run(w, 0.5);
        }
        // 45-я секунда при периоде 20 с: открыт первый выход (фаза 2 из двух выходов -> 0).
        assert_eq!(lane_items(&host, 12, 10).len(), 1);
        assert_eq!(lane_items(&client, 12, 10).len(), 1);
        assert_eq!(lane_items(&client, 11, 11).len(), 0);
        client.sync_time(f64::NAN);
        assert!(client.time.is_finite());
    }

    /// Линия "добыча -> кристаллизатор -> склад-счётчик" и алтарь рядом; возвращает число кристаллов.
    fn altar_line(fps: u32, secs: f32, fuel: Option<Item>) -> u32 {
        let mut w = world();
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(Kind::by_name("КРИСТАЛЛИЗАТОР"), 11, 10, Dir::E).unwrap();
        sink(&mut w, 12, 10, Dir::E);
        w.place(Kind::by_name("АЛТАРЬ"), 11, 12, Dir::N).unwrap();
        for _ in 0..(secs * fps as f32).round() as u32 {
            // Алтарь всё время подкармливается: одно изделие горит, одно ждёт.
            if let Some(it) = fuel {
                w.try_insert(11, 12, it, Dir::E, 0.0);
            }
            w.update(1.0 / fps as f32);
        }
        stored(&w, 12, 10)
    }

    /// Огонь ускоряет машины в радиусе, и результат не зависит от частоты кадров.
    #[test]
    fn altar_fire_speeds_up() {
        let fire = Item::new(1, 3, 25);
        let s = aura_of(fire)[0];
        assert!((s - 0.864).abs() < 0.01, "магистерий огня III даёт +{}", s);
        let mut seen = Vec::new();
        for fps in FPS {
            let (base, fast) = (altar_line(fps, 60.0, None), altar_line(fps, 60.0, Some(fire)));
            // Кристаллизатор: 2.5 с на штуку, с аурой - в 1.864 раза быстрее.
            assert!((23..=24).contains(&base), "{} к/с: без алтаря {}", fps, base);
            assert!((42..=45).contains(&fast), "{} к/с: с алтарём {}", fps, fast);
            seen.push((base, fast));
        }
        for k in 1..seen.len() {
            assert!(seen[k].0.abs_diff(seen[0].0) <= 1 && seen[k].1.abs_diff(seen[0].1) <= 1, "{:?}", seen);
        }
    }

    /// Земля даёт двойной выход с заданной долей - без случайности и одинаково на любом шаге.
    #[test]
    fn altar_earth_doubles_output() {
        let earth = Item::new(4, 3, 25);
        let p = aura_chance(aura_of(earth)[2]);
        assert!((p - 0.432).abs() < 0.005, "шанс {}", p);
        let mut seen = Vec::new();
        for fps in FPS {
            let (base, twice) = (altar_line(fps, 60.0, None), altar_line(fps, 60.0, Some(earth)));
            let want = base as f32 * (1.0 + p);
            assert!((twice as f32 - want).abs() <= 2.0, "{} к/с: {} вместо {:.1}", fps, twice, want);
            seen.push(twice);
        }
        assert!(seen.iter().max().unwrap() - seen.iter().min().unwrap() <= 1, "{:?}", seen);
    }

    /// Вода экономит сырьё: из 10 эссенций выходит 16 кристаллов, сколько бы кадров ни было.
    #[test]
    fn altar_water_saves_input() {
        let water = Item::new(2, 3, 25);
        for fps in FPS {
            for (fuel, want) in [(None, 10), (Some(water), 16)] {
                let mut w = world();
                w.place(Kind::by_name("СКЛАД"), 10, 10, Dir::E).unwrap();
                {
                    let b = w.tiles[(10 * W + 10) as usize].as_mut().unwrap();
                    b.slots[0] = Some(Item::base(0));
                    b.count = 10;
                }
                w.place(Kind::by_name("КРИСТАЛЛИЗАТОР"), 11, 10, Dir::E).unwrap();
                sink(&mut w, 12, 10, Dir::E);
                w.place(Kind::by_name("АЛТАРЬ"), 11, 12, Dir::N).unwrap();
                for _ in 0..90 * fps {
                    if let Some(it) = fuel {
                        w.try_insert(11, 12, it, Dir::E, 0.0);
                    }
                    w.update(1.0 / fps as f32);
                }
                assert_eq!(stored(&w, 12, 10), want, "{} к/с", fps);
            }
        }
    }

    /// Радиус, эфир и правило "ауры одного типа не складываются".
    #[test]
    fn altar_radius_and_no_stacking() {
        let mut w = world();
        let altar = Kind::by_name("АЛТАРЬ");
        w.place(altar, 20, 20, Dir::N).unwrap();
        // Эссенции и ящики алтарь не берёт - только изделия.
        assert!(!w.try_insert(20, 20, Item::base(0), Dir::E, 0.0));
        assert!(!w.try_insert(20, 20, Item::new(1, 1, 4).packed(true), Dir::E, 0.0));
        assert!(w.altars().is_empty() && w.aura_at(20, 20) == [0.0; 3]);
        let weak = Item::new(1, 1, 4);
        assert!(w.try_insert(20, 20, weak, Dir::S, 0.0), "с любой стороны");
        run(&mut w, 0.1);
        let s_weak = aura_of(weak)[0];
        assert!((s_weak - 0.129).abs() < 0.005);
        assert_eq!(w.aura_at(23, 17), [s_weak, 0.0, 0.0], "угол квадрата 7x7");
        assert_eq!(w.aura_at(24, 20), [0.0; 3], "за радиусом");
        // Второй алтарь с сильным огнём: там, где ауры пересекаются, действует сильнейшая.
        let strong = Item::new(1, 3, 25);
        w.place(altar, 24, 20, Dir::N).unwrap();
        assert!(w.try_insert(24, 20, strong, Dir::E, 0.0));
        run(&mut w, 0.1);
        assert_eq!(w.aura_at(22, 20)[0], aura_of(strong)[0], "не сумма, а максимум");
        assert_eq!(w.aura_at(18, 20)[0], s_weak);
        // Смесь делит силу между стихиями: лава (огонь + земля) даёт и скорость, и двойной выход.
        let lava = aura_of(Item::new(5, 3, 25));
        assert!(lava[0] > 0.0 && lava[0] == lava[2] && lava[1] == 0.0 && lava[0] < aura_of(strong)[0]);
        // Чистота - множитель: x1, x1.5, x2.
        let by_p: Vec<f32> = (1..=3).map(|p| aura_of(Item::new(1, p, 25))[0]).collect();
        assert!((by_p[1] / by_p[0] - 1.5).abs() < 1e-4 && (by_p[2] / by_p[0] - 2.0).abs() < 1e-4);
        // Форма - сила: магистерий сильнее слитка.
        assert!(aura_of(Item::new(1, 1, 25))[0] > 3.0 * s_weak);

        // Эфир: плазма (огонь + эфир) расширяет радиус своего алтаря.
        let mut w = world();
        w.place(altar, 20, 20, Dir::N).unwrap();
        let plasma = Item::new(9, 3, 25);
        assert!(w.try_insert(20, 20, plasma, Dir::E, 0.0));
        run(&mut w, 0.1);
        let r = ALTAR_RADIUS + aura_radius_bonus(aura_of(plasma)[3]);
        assert_eq!((r, w.altars()[0].r), (6, 6));
        assert!(w.aura_at(26, 26)[0] > 0.0 && w.aura_at(27, 20)[0] == 0.0);
        // Алтарь с чистым эфиром сам машинам ничего не даёт, но расширяет соседний алтарь.
        let mut w = world();
        w.place(altar, 20, 20, Dir::N).unwrap();
        w.place(altar, 22, 20, Dir::N).unwrap();
        assert!(w.try_insert(20, 20, strong, Dir::E, 0.0));
        run(&mut w, 0.1);
        assert_eq!(w.aura_at(25, 20), [0.0; 3]);
        assert!(w.try_insert(22, 20, Item::new(8, 3, 25), Dir::E, 0.0));
        run(&mut w, 0.1);
        let info = w.altars();
        assert_eq!(info.iter().find(|a| a.x == 20).unwrap().r, 8);
        assert!(w.aura_at(28, 20)[0] > 0.0 && w.aura_at(29, 20) == [0.0; 3]);
        // Снос алтаря снимает ауру.
        assert!(w.remove(20, 20));
        run(&mut w, 0.1);
        assert_eq!(w.aura_at(21, 20), [0.0; 3]);
    }

    /// Изделие горит ровно ALTAR_BURN секунд на любой частоте кадров; следующее берётся из очереди.
    #[test]
    fn altar_burns_on_schedule() {
        for fps in FPS {
            let mut w = world();
            w.place(Kind::by_name("АЛТАРЬ"), 20, 20, Dir::N).unwrap();
            let (a, b) = (Item::new(1, 1, 4), Item::new(2, 1, 4));
            assert!(w.try_insert(20, 20, a, Dir::E, 0.0));
            assert!(w.try_insert(20, 20, b, Dir::E, 0.0));
            assert!(!w.try_insert(20, 20, b, Dir::E, 0.0), "очередь - одно изделие");
            run_fps(&mut w, ALTAR_BURN - 0.5, fps);
            assert!(w.aura_at(20, 21)[0] > 0.0 && w.aura_at(20, 21)[1] == 0.0, "{} к/с", fps);
            run_fps(&mut w, 1.0, fps);
            assert!(w.aura_at(20, 21)[0] == 0.0 && w.aura_at(20, 21)[1] > 0.0, "{} к/с: горит второе", fps);
            run_fps(&mut w, ALTAR_BURN - 1.0, fps);
            assert!(w.aura_at(20, 21)[1] > 0.0, "{} к/с", fps);
            run_fps(&mut w, 1.0, fps);
            assert_eq!(w.aura_at(20, 21), [0.0; 3], "{} к/с: всё сгорело", fps);
            assert!(w.altars().is_empty());
        }
    }

    /// Подаёт предмет в заднюю сторону машины, смотрящей на восток.
    fn feed(w: &mut World, x: i32, y: i32, it: Item) -> bool {
        w.try_insert(x, y, it, Dir::E, 0.0)
    }

    /// Машина на (11, 10) лицом на восток, перед ней лента, справа (на юге) - лента для отказа.
    fn bench(name: &str) -> World {
        let mut w = world();
        w.place(Kind::by_name(name), 11, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 12, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        w
    }

    /// Назначает правую сторону машины на (11, 10) стороной отказа.
    fn reject_right(w: &mut World) {
        assert!(w.set_cfg(11, 10, 2));
        assert_eq!(reject_rel(w.get(11, 10).unwrap()), Some(1));
    }

    /// Тигель сливает два изделия одной формы без общих стихий; чистота - меньшая из двух.
    #[test]
    fn crucible_merges_mixes() {
        for fps in FPS {
            let mut w = bench("ТИГЕЛЬ");
            assert!(feed(&mut w, 11, 10, Item::new(1, 3, 4)), "слиток огня III");
            assert!(!feed(&mut w, 11, 10, Item::new(1, 1, 4)), "та же стихия");
            assert!(!feed(&mut w, 11, 10, Item::new(3, 1, 4)), "пар содержит огонь");
            assert!(!feed(&mut w, 11, 10, Item::new(2, 1, 5)), "другая форма");
            assert!(!feed(&mut w, 11, 10, Item::base(1)), "эссенция - не изделие");
            assert!(feed(&mut w, 11, 10, Item::new(2, 1, 4)), "слиток воды I");
            run_fps(&mut w, 2.5, fps);
            assert!(lane_items(&w, 12, 10).is_empty(), "{} к/с: плавка идёт 3 с", fps);
            run_fps(&mut w, 1.0, fps);
            assert_eq!(lane_items(&w, 12, 10), vec![Item::new(3, 1, 4)], "{} к/с: слиток пара I", fps);
        }
        // Слитую смесь можно сливать дальше, пока стихии не пересекаются.
        let mut w = bench("ТИГЕЛЬ");
        assert!(feed(&mut w, 11, 10, Item::new(3, 2, 7)) && feed(&mut w, 11, 10, Item::new(12, 2, 7)));
        run(&mut w, 3.5);
        assert_eq!(lane_items(&w, 12, 10), vec![Item::new(15, 2, 7)], "пар + пыль = квинтэссенция");
        // Квинтэссенцию и философский камень сливать не с чем: тигель их не берёт вовсе.
        assert!(!feed(&mut w, 11, 10, Item::new(15, 1, 7)));
        assert!(!feed(&mut w, 11, 10, Item::new(15, 1, FORM_STONE)));
        assert!(!feed(&mut w, 11, 10, Item::new(1, 1, 4).packed(true)));
        // Со стороной отказа неподходящее не запирает вход, а уходит вбок.
        reject_right(&mut w);
        assert!(feed(&mut w, 11, 10, Item::new(1, 1, 4)));
        assert!(feed(&mut w, 11, 10, Item::new(5, 1, 4)), "лава не сочетается с огнём - в отказ");
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 11, 11), vec![Item::new(5, 1, 4)]);
        assert!(feed(&mut w, 11, 10, Item::new(8, 1, 4)));
        run(&mut w, 3.5);
        assert_eq!(lane_items(&w, 12, 10).last(), Some(&Item::new(9, 1, 4)));
    }

    /// Трансмутатор: две эссенции стихии -> одна следующей по кругу огонь > земля > вода > эфир > огонь.
    #[test]
    fn transmuter_turns_the_wheel() {
        for fps in FPS {
            for (from, to) in [(1u8, 4u8), (4, 2), (2, 8), (8, 1)] {
                let mut w = bench("ТРАНСМУТАТОР");
                assert!(feed(&mut w, 11, 10, Item::new(from, 2, 0)));
                assert!(!feed(&mut w, 11, 10, Item::new(to, 2, 0)), "вторая эссенция - той же стихии");
                assert!(feed(&mut w, 11, 10, Item::new(from, 1, 0)));
                run_fps(&mut w, 3.5, fps);
                assert_eq!(lane_items(&w, 12, 10), vec![Item::new(to, 1, 0)], "{} к/с: {} -> {}", fps, from, to);
            }
        }
        assert_eq!((0..4).fold(1u8, |m, _| next_element(m)), 1, "круг замыкается за четыре шага");
        let mut w = bench("ТРАНСМУТАТОР");
        assert!(!feed(&mut w, 11, 10, Item::new(3, 1, 0)), "смесь не трансмутируется");
        assert!(!feed(&mut w, 11, 10, Item::new(1, 1, 1)), "изделие не трансмутируется");
        reject_right(&mut w);
        assert!(feed(&mut w, 11, 10, Item::new(3, 1, 0)));
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 11, 11), vec![Item::new(3, 1, 0)]);
    }

    /// Атанор собирает философский камень из магистериев четырёх чистых стихий.
    #[test]
    fn opus_makes_philosophers_stone() {
        let mag = |e: u8, p: u8| Item::new(1 << e, p, FORM_MAGISTERIUM);
        for fps in FPS {
            let mut w = bench("АТАНОР");
            // Вперемешку по одной ленте: у каждой стихии своя ячейка, вход не запирается.
            for it in [mag(0, 3), mag(0, 3), mag(1, 2), mag(3, 3), mag(0, 3), mag(2, 3), mag(0, 3)] {
                assert!(feed(&mut w, 11, 10, it), "{:?}", it);
            }
            assert_eq!(w.get(11, 10).unwrap().bins, [4, 1, 1, 1]);
            assert!(!feed(&mut w, 11, 10, mag(0, 3)), "ячейка огня полна - пятый ждёт");
            assert!(!feed(&mut w, 11, 10, Item::new(3, 3, FORM_MAGISTERIUM)), "нужны чистые стихии");
            assert!(!feed(&mut w, 11, 10, Item::new(1, 3, 24)), "нужен магистерий");
            run_fps(&mut w, 7.5, fps);
            assert!(lane_items(&w, 12, 10).is_empty(), "{} к/с: камень делается 8 с", fps);
            run_fps(&mut w, 1.0, fps);
            let stone = Item::new(15, 2, FORM_STONE);
            assert_eq!(lane_items(&w, 12, 10), vec![stone], "{} к/с: чистота - меньшая из четырёх", fps);
            assert_eq!(w.get(11, 10).unwrap().bins, [3, 0, 0, 0]);
            // Ещё два полных набора -> ровно два камня, на любой частоте кадров.
            w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 12, 10, Dir::N).unwrap();
            for e in 1..4 {
                assert!(feed(&mut w, 11, 10, mag(e, 3)) && feed(&mut w, 11, 10, mag(e, 3)));
            }
            run_fps(&mut w, 20.0, fps);
            assert_eq!((w.total_sold, w.get(11, 10).unwrap().bins), (2, [1, 0, 0, 0]), "{} к/с", fps);
        }
        // Камень: имя, цена (больше четырёх магистериев квинтэссенции) и место вне сетки каталога.
        let stone = Item::new(15, 3, FORM_STONE);
        assert_eq!(stone.name(), "ФИЛОСОФСКИЙ КАМЕНЬ III");
        assert_eq!(stone.value(), 120.0 * 10.0 * 2000.0);
        assert!(stone.value() > 4.0 * Item::new(15, 3, FORM_MAGISTERIUM).value());
        assert!(stone.id() >= N_ITEM_IDS && stone.id() < N_ALL_IDS);
        assert_eq!(form_recipe(FORM_STONE).0, "АТАНОР");
        assert!(aura_of(stone).iter().all(|s| *s > 0.0), "камень на алтаре даёт все четыре ауры");
        // Сторона отказа: лишний магистерий и чужие предметы не запирают вход.
        let mut w = bench("АТАНОР");
        reject_right(&mut w);
        for _ in 0..4 {
            assert!(feed(&mut w, 11, 10, mag(0, 1)));
        }
        assert!(feed(&mut w, 11, 10, mag(0, 1)), "пятый огонь уходит в отказ");
        run(&mut w, 1.0);
        assert!(feed(&mut w, 11, 10, Item::new(1, 1, 4)));
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 11, 11), vec![mag(0, 1), Item::new(1, 1, 4)]);
        let back = World::load(&w.save()).unwrap();
        assert_eq!(reject_rel(back.get(11, 10).unwrap()), Some(1));
    }

    /// Склад-источник: выдаёт вперёд `n` одинаковых предметов.
    fn source(w: &mut World, x: i32, y: i32, d: Dir, it: Item, n: u32) {
        w.place(Kind::by_name("СКЛАД"), x, y, d).unwrap();
        let b = w.tiles[(y * W + x) as usize].as_mut().unwrap();
        b.slots[0] = Some(it);
        b.count = n;
    }

    /// Упаковщик собирает 8 одинаковых предметов в ящик, распаковщик разбирает обратно.
    #[test]
    fn packer_and_unpacker() {
        let it = Item::new(3, 2, 4);
        for fps in FPS {
            let mut w = world();
            source(&mut w, 10, 10, Dir::E, it, 20);
            w.place(Kind::by_name("УПАКОВЩИК"), 11, 10, Dir::E).unwrap();
            sink(&mut w, 12, 10, Dir::E);
            run_fps(&mut w, 10.0, fps);
            // Из 20 штук - два ящика, четыре штуки ждут в упаковщике.
            let st = w.get(12, 10).unwrap();
            assert_eq!((stored(&w, 12, 10), st.slots[0]), (2, Some(it.packed(true))), "{} к/с", fps);
            assert_eq!(w.get(11, 10).unwrap().count, 4);
            // Ящики едут в распаковщик и снова становятся штуками.
            let mut w = world();
            source(&mut w, 10, 10, Dir::E, it.packed(true), 3);
            w.place(Kind::by_name("РАСПАКОВЩИК"), 11, 10, Dir::E).unwrap();
            sink(&mut w, 12, 10, Dir::E);
            run_fps(&mut w, 20.0, fps);
            assert_eq!((stored(&w, 12, 10), w.get(12, 10).unwrap().slots[0]), (24, Some(it)), "{} к/с", fps);
        }
        let crate_ = it.packed(true);
        assert_eq!((crate_.pieces(), crate_.value(), crate_.name()), (8, it.value() * 8.0, format!("ЯЩИК: {}", it.name())));
        // Чужой предмет при начатом ящике: без стороны отказа ждёт, со стороной - уходит вбок.
        let mut w = bench("УПАКОВЩИК");
        assert!(feed(&mut w, 11, 10, it));
        assert!(!feed(&mut w, 11, 10, Item::new(1, 1, 4)));
        assert!(!feed(&mut w, 11, 10, crate_), "ящик в ящик не кладётся");
        reject_right(&mut w);
        assert!(feed(&mut w, 11, 10, Item::new(1, 1, 4)));
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 11, 11), vec![Item::new(1, 1, 4)]);
        let mut w = bench("РАСПАКОВЩИК");
        assert!(!feed(&mut w, 11, 10, it), "распаковщик берёт только ящики");
        assert!(feed(&mut w, 11, 10, crate_) && !feed(&mut w, 11, 10, crate_));
        // Машины и алтарь ящики не берут.
        let mut w = bench("КРИСТАЛЛИЗАТОР");
        assert!(!feed(&mut w, 11, 10, Item::base(0).packed(true)));
    }

    /// Лавка принимает ящик как 8 продаж: каталог, контракт, счётчики и статистика - по штукам.
    #[test]
    fn crate_sells_as_eight() {
        let mut w = world();
        let want = contract(w.level).item;
        let money = w.money;
        assert!(w.try_insert(HUB_X, HUB_Y, want.packed(true), Dir::E, 0.0));
        assert_eq!((w.total_sold, w.delivered, w.sold_by_id[want.id()]), (8, 8, 8));
        assert_eq!(w.recent_sales(), vec![(want.id(), 8)]);
        assert_eq!(w.pending_new.len(), 1, "новинка каталога - одна");
        let eight: f64 = (0..8).map(|k| want.value() * market_curve(k as f64)).sum();
        assert!((w.money - (money + eight + want.first_sale_bonus())).abs() < 1e-3);
        // Торговый пост тоже продаёт ящик поштучно, но в контракт и статистику лавки не идёт.
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 5, 5, Dir::N).unwrap();
        assert!(w.try_insert(5, 5, want.packed(true), Dir::E, 0.0));
        assert_eq!((w.total_sold, w.delivered, w.sold_by_id[want.id()]), (16, 8, 8));
    }

    /// Телепорт - кроличья нора: общий код, приёмники по очереди, всё вошедшее выходит через секунду.
    #[test]
    fn teleport_channel() {
        let (tx, rx) = (Kind::by_name("ТЕЛЕПОРТ-ПЕРЕДАТЧИК"), Kind::by_name("ТЕЛЕПОРТ-ПРИЁМНИК"));
        let it = Item::new(6, 1, 7);
        for fps in FPS {
            let mut w = world();
            // Два передатчика на одном коде, у каждого по 100 предметов.
            for y in [10, 14] {
                source(&mut w, 10, y, Dir::E, it, 100);
                w.place(tx, 11, y, Dir::N).unwrap();
                assert!(w.set_cfg(11, y, tele_cfg(42)));
            }
            for y in [40, 50] {
                w.place(rx, 60, y, Dir::E).unwrap();
                assert!(w.set_cfg(60, y, tele_cfg(42)));
                sink(&mut w, 61, y, Dir::E);
            }
            // Приёмник с другим кодом и приёмник без кода ничего не получают.
            w.place(rx, 60, 60, Dir::E).unwrap();
            w.set_cfg(60, 60, tele_cfg(43));
            sink(&mut w, 61, 60, Dir::E);
            w.place(rx, 60, 70, Dir::E).unwrap();
            sink(&mut w, 61, 70, Dir::E);
            run_fps(&mut w, 0.9, fps);
            assert_eq!(stored(&w, 61, 40) + stored(&w, 61, 50), 0, "{} к/с: перелёт длится секунду", fps);
            // Пропускная способность не ограничена: за 20 с проходят все 200 предметов.
            run_fps(&mut w, 19.1, fps);
            let (a, b) = (stored(&w, 61, 40), stored(&w, 61, 50));
            assert_eq!(a + b, 200, "{} к/с", fps);
            assert!(a.abs_diff(b) <= 2, "{} к/с: приёмники получают по очереди: {} и {}", fps, a, b);
            assert_eq!((stored(&w, 61, 60), stored(&w, 61, 70)), (0, 0));
            assert_eq!(w.tele_receivers(42), 2);
            // Код переживает сохранение.
            let back = World::load(&w.save()).unwrap();
            assert_eq!(tele_code(back.get(11, 10).unwrap().cfg), Some(42));
            assert_eq!(tele_code(back.get(60, 60).unwrap().cfg), Some(43));
            assert_eq!(tele_code(back.get(60, 70).unwrap().cfg), None);

            // Один предмет: через 0.9 с его ещё нет, через 1.1 с он уже у приёмника.
            let mut w = world();
            w.place(tx, 11, 10, Dir::N).unwrap();
            w.set_cfg(11, 10, tele_cfg(0));
            w.place(rx, 30, 30, Dir::E).unwrap();
            w.set_cfg(30, 30, tele_cfg(0));
            w.place(Kind::Belt, 31, 30, Dir::E).unwrap();
            assert!(w.try_insert(11, 10, it, Dir::S, 0.0), "с любой стороны");
            run_fps(&mut w, 0.9, fps);
            assert!(lane_items(&w, 31, 30).is_empty() && w.get(30, 30).unwrap().out.is_none(), "{} к/с", fps);
            run_fps(&mut w, 0.2, fps);
            assert_eq!(lane_items(&w, 31, 30), vec![it], "{} к/с", fps);
        }
        // Код 0000 - настоящий код, а не "не задан"; мусор отбрасывается.
        assert_eq!((tele_code(tele_cfg(0)), tele_code(0), tele_code(tele_cfg(9999))), (Some(0), None, Some(9999)));
        assert_eq!(clean_cfg(Role::TeleIn, 123_456), 0);
        // Без приёмника предметы ждут в норе и не теряются; ящик летит одним предметом.
        let mut w = world();
        w.place(tx, 11, 10, Dir::N).unwrap();
        w.set_cfg(11, 10, tele_cfg(7));
        assert!(w.try_insert(11, 10, it.packed(true), Dir::S, 0.0) && w.try_insert(11, 10, it, Dir::E, 0.0));
        run(&mut w, 5.0);
        assert_eq!(lane_items(&w, 11, 10), vec![it.packed(true), it]);
        w.place(rx, 30, 30, Dir::E).unwrap();
        w.set_cfg(30, 30, tele_cfg(7));
        w.place(Kind::Belt, 31, 30, Dir::E).unwrap();
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 31, 30), vec![it.packed(true), it]);
        // Приёмник уже, чем поток: четыре передатчика одного кода отдают ему предметы по очереди.
        let mut w = world();
        for e in 0..4u8 {
            source(&mut w, 10 + 2 * e as i32, 10, Dir::S, Item::base(e), 1000);
            w.place(tx, 10 + 2 * e as i32, 11, Dir::N).unwrap();
            w.set_cfg(10 + 2 * e as i32, 11, tele_cfg(5));
        }
        w.place(rx, 30, 30, Dir::E).unwrap();
        w.set_cfg(30, 30, tele_cfg(5));
        for x in 31..43 {
            w.place(Kind::Belt, x, 30, Dir::E).unwrap();
        }
        run(&mut w, 8.0);
        let mut seen = [0u32; 4];
        for x in 31..43 {
            for it in lane_items(&w, x, 30) {
                seen[it.mask.trailing_zeros() as usize] += 1;
            }
        }
        assert!(seen.iter().all(|n| *n >= 5 && n.abs_diff(seen[0]) <= 1), "{:?}", seen);
        // Нора не бездонная: при занятых приёмниках передатчик держит не больше TELE_QUEUE предметов.
        let mut w = world();
        w.place(tx, 11, 10, Dir::N).unwrap();
        let taken = (0..100).filter(|_| w.try_insert(11, 10, it, Dir::E, 0.0)).count();
        assert_eq!(taken, TELE_QUEUE);
    }

    /// Сортировочный узел: один вход, три выхода, у каждого своё условие.
    #[test]
    fn sorter_routes_by_conditions() {
        let node = Kind::by_name("СОРТИРОВОЧНЫЙ УЗЕЛ");
        let mut w = world();
        w.place(node, 11, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 12, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        w.place(Kind::Belt, 11, 9, Dir::N).unwrap();
        // Вперёд - чистота II, вправо - слитки, влево - всё остальное.
        let mut cfg = sorter_set(0, 0, Cond { purity: 2, ..Cond::default() });
        cfg = sorter_set(cfg, 1, Cond { form: 4 + 1, ..Cond::default() });
        assert!(w.set_cfg(11, 10, cfg));
        let items = [Item::new(1, 2, 4), Item::new(2, 1, 4), Item::new(3, 1, 0), Item::new(9, 3, 4).packed(true)];
        for it in items {
            assert!(feed(&mut w, 11, 10, it));
            run(&mut w, 1.0);
        }
        // Слиток II подходит двум условиям - уходит в первый подходящий выход.
        assert_eq!(lane_items(&w, 12, 10), vec![items[0]]);
        assert_eq!(lane_items(&w, 11, 11), vec![items[1], items[3]]);
        assert_eq!(lane_items(&w, 11, 9), vec![items[2]]);
        let saved = w.save();
        let back = World::load(&saved).unwrap();
        assert_eq!(back.get(11, 10).unwrap().cfg, cfg);
        assert_eq!((sorter_cond(cfg, 0).purity, sorter_cond(cfg, 1).form, sorter_cond(cfg, 2).is_any()), (2, 5, true));
        // Условие "в ящике" на третьем выходе; теперь "остального" выхода нет - неподходящее ждёт.
        let cfg = sorter_set(cfg, 2, Cond { boxed: 2, ..Cond::default() });
        w.set_cfg(11, 10, cfg);
        assert!(feed(&mut w, 11, 10, Item::new(3, 1, 0)));
        run(&mut w, 2.0);
        assert_eq!(w.get(11, 10).unwrap().slots[0], Some(Item::new(3, 1, 0)));
        // Без условий узел работает как перелив: вперёд, а если занято - вбок.
        let mut w = world();
        w.place(node, 11, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 11, Dir::S).unwrap();
        assert!(feed(&mut w, 11, 10, Item::base(0)));
        run(&mut w, 1.0);
        assert_eq!(lane_items(&w, 11, 11), vec![Item::base(0)]);
    }

    /// Каталог различает чистоту: 1170 позиций, своя премия у I, II и III; старая строка D читается.
    #[test]
    fn catalog_by_purity() {
        assert_eq!((N_ITEM_IDS, N_KINDS), (1170, 390));
        let mut w = world();
        let (i1, i2, i3) = (Item::new(5, 1, 7), Item::new(5, 2, 7), Item::new(5, 3, 7));
        let mut bonus = Vec::new();
        for it in [i1, i1, i3, i2, i2] {
            let before = w.money;
            assert!(w.try_insert(HUB_X, HUB_Y, it, Dir::E, 0.0));
            bonus.push(w.money - before - it.value());
        }
        // Вторая такая же штука идёт чуть дешевле (насыщение рынка), премии - только за первую.
        let dip = market_curve(1.0) - 1.0;
        let want = [i1.value() * 10.0, i1.value() * dip, i3.value() * 10.0, i2.value() * 10.0, i2.value() * dip];
        assert!(bonus.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3), "{:?}", bonus);
        assert_eq!((w.catalog_found(), w.catalog_missing().len()), (3, 1167));
        assert!(w.kind_known(i1.kind_idx()) && !w.kind_known(0));
        assert_eq!(Item::from_id(i3.id()), i3);
        // Список недостающего идёт от дешёвого к дорогому и не содержит проданного.
        let miss = w.catalog_missing();
        assert_eq!(miss[0], Item::base(0));
        assert!(miss.windows(2).all(|p| p[0].value() <= p[1].value()) && !miss.contains(&i2));
        // Статистика: продажи по позициям за последние минуты; старое уходит из окна.
        assert_eq!(w.recent_sales(), vec![(i1.id(), 2), (i2.id(), 2), (i3.id(), 1)]);
        run(&mut w, 120.0);
        assert!(w.try_insert(HUB_X, HUB_Y, i3, Dir::E, 0.0));
        assert_eq!(w.recent_sales()[0], (i1.id(), 2));
        run(&mut w, 60.0 * STAT_MINUTES as f32);
        assert!(w.recent_sales().is_empty());
        // Круг сохранения не теряет каталог; философский камень хранится там же.
        assert!(w.try_insert(HUB_X, HUB_Y, Item::new(15, 2, FORM_STONE), Dir::E, 0.0));
        assert_eq!(w.catalog_found(), 3, "камень вне сетки каталога");
        let saved = w.save();
        let back = World::load(&saved).unwrap();
        assert_eq!(back.discovered, w.discovered);
        assert_eq!(back.save(), saved);
        // Старое сохранение: строка D из 390 символов без чистоты - вид засчитывается как чистота I.
        let mut d = vec!['0'; 390];
        d[i1.kind_idx()] = '1';
        d[389] = '1';
        let old = format!("ALEMBIC1\n500 3 0 0 0 0 0 9\nD {}\nU 0 0 0 0\n10 10 0 1 1\n", d.iter().collect::<String>());
        let w = World::load(&old).expect("старое сохранение читается");
        assert!(w.discovered[i1.id()] && !w.discovered[i2.id()] && !w.discovered[i3.id()]);
        assert!(w.discovered[Item::new(15, 1, 25).id()]);
        assert_eq!((w.catalog_found(), w.get(10, 10).map(|b| b.kind)), (2, Some(Kind::Belt)));
        // Новый файл по-прежнему содержит строку D в старом виде - её поймёт прежняя версия игры.
        let line = w.save().lines().find(|l| l.starts_with("D ")).unwrap().to_string();
        assert_eq!(line.len(), 2 + 390);
        assert_eq!(line.matches('1').count(), 2);
    }

    /// Сохранение со всеми новыми постройками и настройками переживает save -> load без потерь.
    #[test]
    fn new_buildings_roundtrip() {
        let mut w = world();
        let names = [
            "ДЕЛИТЕЛЬ С ПРОПОРЦИЕЙ", "АЛТАРЬ", "ТИГЕЛЬ", "ТРАНСМУТАТОР", "АТАНОР", "УПАКОВЩИК", "РАСПАКОВЩИК",
            "ТЕЛЕПОРТ-ПЕРЕДАТЧИК", "ТЕЛЕПОРТ-ПРИЁМНИК", "СОРТИРОВОЧНЫЙ УЗЕЛ", "ФИЛЬТР", "ДИСТИЛЛЯТОР", "ПЕРЕКЛЮЧАТЕЛЬ",
        ];
        for (i, n) in names.iter().enumerate() {
            w.place(Kind::by_name(n), 5 + 2 * i as i32, 5, Dir::from_u8(i as u8)).unwrap();
        }
        // Номера новых построек постоянны: они идут в конце таблицы, перед лавкой.
        let ids: Vec<u8> = names[..10].iter().map(|n| Kind::by_name(n).0).collect();
        assert_eq!(ids, vec![40, 41, 42, 43, 44, 45, 46, 47, 48, 49]);
        assert_eq!((Kind::by_name("ПЕРЕКЛЮЧАТЕЛЬ").0, Kind::Hub.0 as usize, KINDS.len()), (39, KINDS.len() - 1, KINDS.len()));
        let filter_cond = Cond { mask: 7, purity: 3, form: FORM_STONE + 1, boxed: 2 };
        let sort = sorter_set(sorter_set(0, 0, Cond { mask: 15, ..Cond::default() }), 2, filter_cond);
        let cfgs = [(5, ratio_cfg(7, 2)), (9, 3), (11, 4), (13, 1), (15, 2), (17, 3), (19, tele_cfg(0)), (21, tele_cfg(9999)), (23, sort), (25, filter_cond.bits()), (27, 4)];
        for (x, cfg) in cfgs {
            assert!(w.set_cfg(x, 5, cfg), "постройка на {}", x);
            assert_eq!(w.get(x, 5).unwrap().cfg, cfg, "постройка на {}", x);
        }
        w.force_level(29, 5, 4);
        w.force_level(13, 5, 5);
        run(&mut w, 3.3);
        let saved = w.save();
        let back = World::load(&saved).expect("загружается");
        assert_eq!(back.save(), saved);
        for i in 0..names.len() as i32 {
            let (a, b) = (w.get(5 + 2 * i, 5).unwrap(), back.get(5 + 2 * i, 5).unwrap());
            assert_eq!((a.kind, a.dir, a.level, a.ports, a.cfg), (b.kind, b.dir, b.level, b.ports, b.cfg), "{}", names[i as usize]);
        }
        assert_eq!(ratio_nm(back.get(5, 5).unwrap().cfg), (7, 2));
        assert_eq!(Cond::from_bits(back.get(25, 5).unwrap().cfg), filter_cond);
        assert_eq!(sorter_cond(back.get(23, 5).unwrap().cfg, 2), filter_cond);
        assert!((back.time - w.time).abs() < 0.002);
        // Постройки без настроек пишутся в прежнем виде, без лишних чисел.
        assert!(saved.contains(&format!("7 5 {} 1 1\n", Kind::by_name("АЛТАРЬ").0)));
    }

    // ---------- рынок, гильдии, мегапроект, виток, карта ----------

    fn hub(w: &mut World, it: Item) {
        assert!(w.try_insert(HUB_X, HUB_Y, it, Dir::E, 0.0));
    }

    /// Насыщение рынка: цена падает от продаж и восстанавливается; "всего понемногу" выгоднее одной линии.
    #[test]
    fn market_saturation() {
        // Кривая: вдвое дешевле при затоваренности 60, пол - четверть цены.
        assert_eq!((market_curve(0.0), market_curve(60.0), market_curve(1e6)), (1.0, 0.5, MARKET_FLOOR));
        let fire = Item::base(0);
        let mut w = world();
        for _ in 0..60 {
            hub(&mut w, fire);
        }
        assert!((w.market_factor(fire) - 0.5).abs() < 1e-6);
        assert_eq!(w.market_factor(Item::base(1)), 1.0, "у каждой позиции свой рынок");
        assert_eq!(w.market_factor(Item::new(1, 2, 0)), 1.0, "чистота II - другая позиция");
        // Восстановление: через MARKET_TAU секунд затоваренность меньше в e раз - на любой частоте кадров.
        for fps in FPS {
            let mut w = world();
            (0..60).for_each(|_| hub(&mut w, fire));
            run_fps(&mut w, MARKET_TAU as f32, fps);
            let want = market_curve(60.0 / std::f64::consts::E);
            assert!((w.market_factor(fire) - want).abs() < 0.003, "{} к/с: {} вместо {}", fps, w.market_factor(fire), want);
            run_fps(&mut w, 600.0, fps);
            assert!(w.market_factor(fire) > 0.99, "рынок восстановился");
        }
        // Одна линия против четырёх: 240 штук одной позиции или по 60 штук четырёх равноценных.
        let earn = |items: &[Item]| {
            let mut w = world();
            w.discovered.iter_mut().for_each(|d| *d = true);
            let m0 = w.money;
            for k in 0..240 {
                hub(&mut w, items[k % items.len()]);
            }
            w.money - m0
        };
        let (one, four) = (earn(&[fire]), earn(&[Item::base(0), Item::base(1), Item::base(2), Item::base(3)]));
        assert!(four > 1.5 * one, "разнообразие выгоднее: {} против {}", four, one);
        // Рынок пишется в сохранение и не сбрасывается загрузкой.
        let mut w = world();
        (0..100).for_each(|_| hub(&mut w, fire));
        let back = World::load(&w.save()).unwrap();
        assert!((back.market_factor(fire) - w.market_factor(fire)).abs() < 1e-4);
        assert_eq!(back.save(), w.save());
        // Торговый пост давит на тот же рынок.
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 5, 5, Dir::N).unwrap();
        let before = w.market_factor(fire);
        assert!(w.try_insert(5, 5, fire, Dir::E, 0.0) && w.market_factor(fire) < before);
    }

    /// Гильдии: три предложения, заказ со сроком, репутация и награды за неё.
    #[test]
    fn guild_orders_and_rewards() {
        let mut w = world();
        w.level = 5;
        run(&mut w, 0.1);
        assert_eq!(w.offers.len(), 3);
        let same = {
            let mut v = world();
            v.level = 5;
            run(&mut v, 0.1);
            v.offers.clone()
        };
        assert_eq!(w.offers, same, "предложения зависят только от сида и счётчика");
        assert!(w.offers.iter().all(|o| o.item.mask >> o.guild & 1 == 1 && o.need >= 15 && o.reward > 0.0));
        let o = w.offers[1];
        assert_eq!(w.take_order(1), Ok(()));
        assert!(w.take_order(0).is_err(), "заказ один");
        assert_eq!(w.offers.len(), 3, "появились новые предложения");
        let (money, rep) = (w.money, w.rep[o.guild as usize]);
        for _ in 0..o.need - 1 {
            hub(&mut w, o.item);
        }
        assert_eq!(w.order.map(|x| x.1), Some(o.need - 1));
        hub(&mut w, Item::new(o.item.mask, 3, o.item.form));
        assert!(w.order.is_some(), "не тот предмет заказ не закрывает");
        hub(&mut w, o.item);
        assert!(w.order.is_none() && w.rep[o.guild as usize] == rep + o.rep);
        assert!(w.money > money + o.reward, "награда выплачена");
        // Просроченный заказ пропадает без награды.
        w.take_order(0).unwrap();
        let (secs, rep) = (w.order.unwrap().0.secs, w.rep);
        run(&mut w, secs as f32 + 1.0);
        assert!(w.order.is_none() && w.rep == rep);
        assert!(w.pending_msgs.iter().any(|m| m.contains("ПРОСРОЧЕН")));

        // Награды: 3 - скин лент, 6 - жила чистоты II, 10 - гильдейская лавка.
        let mut w = world();
        let post = Kind::by_name("ГИЛЬДЕЙСКАЯ ЛАВКА");
        assert!(!w.set_skin(1) && !w.unlocked(post));
        w.add_rep(0, 3);
        assert!(w.set_skin(1) && !w.set_skin(2) && w.skin == 1);
        assert!(!w.rich.iter().any(|r| *r));
        w.add_rep(0, 3);
        let n = w.rich.iter().filter(|r| **r).count();
        assert!(n >= 6, "жила огня у лавки стала богатой: {} клеток", n);
        let i = w.rich.iter().position(|r| *r).unwrap() as i32;
        assert_eq!(w.veins[i as usize], 1);
        w.place(Kind::Extractor, i % W, i / W, Dir::E).unwrap();
        w.place(Kind::by_name("СКЛАД"), i % W + 1, i / W, Dir::E).unwrap();
        run(&mut w, 3.0);
        assert_eq!(w.get(i % W + 1, i / W).unwrap().slots[0], Some(Item::new(1, 2, 0)), "добыча чистоты II");
        w.add_rep(0, 4);
        assert!(w.unlocked(post));
        // Гильдейская лавка продаёт как лавка алхимика: полная цена и контракт.
        w.place(post, 5, 5, Dir::N).unwrap();
        let want = contract(w.level).item;
        let money = w.money;
        assert!(w.try_insert(5, 5, want, Dir::E, 0.0));
        assert_eq!((w.delivered, w.money), (1, money + want.value() + want.first_sale_bonus()));
        // Всё это переживает сохранение.
        w.take_order(2).unwrap();
        let saved = w.save();
        let back = World::load(&saved).unwrap();
        assert_eq!((back.rep, back.skin, back.offers.clone()), (w.rep, w.skin, w.offers.clone()));
        assert_eq!(back.order.map(|o| (o.0, o.1, o.2.round())), w.order.map(|o| (o.0, o.1, o.2.round())));
        assert_eq!(back.rich, w.rich);
        assert_eq!(back.save(), saved);
    }

    /// Великое Делание и новый виток.
    #[test]
    fn great_work_and_new_cycle() {
        let mut w = world();
        assert!(w.prestige().is_err(), "без камня витка нет");
        for id in 0..N_ITEM_IDS - 1 {
            hub(&mut w, Item::from_id(id));
        }
        assert!(!w.great_done && w.great.iter().filter(|g| **g).count() == N_ITEM_IDS - 1);
        // Торговый пост в мегапроект не идёт - только лавка.
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 5, 5, Dir::N).unwrap();
        let last = Item::from_id(N_ITEM_IDS - 1);
        assert!(w.try_insert(5, 5, last, Dir::E, 0.0) && !w.great_done);
        let money = w.money;
        hub(&mut w, last);
        assert!(w.great_done && w.money > money + GREAT_REWARD);
        hub(&mut w, last);
        assert!(w.money < money + GREAT_REWARD * 1.5, "награда одна");
        // Камни: сданные в лавку копятся, виток превращает их в постоянные бонусы.
        let stone = Item::new(15, 1, FORM_STONE);
        (0..3).for_each(|_| hub(&mut w, stone));
        assert_eq!((w.stones, w.stones_now), (0, 3));
        w.place(Kind::Belt, 10, 10, Dir::E).unwrap();
        lg(&mut w, "ТАЙМЕР", 10, 10, Dir::E);
        w.upgrades[3] = 5;
        let (speed, found, rep) = (w.machine_speed(), w.catalog_found(), w.rep);
        let saved = w.save();
        assert_eq!(World::load(&saved).unwrap().save(), saved);
        assert_eq!(w.prestige(), Ok(3));
        assert_eq!((w.cycle, w.stones, w.stones_now, w.level, w.upgrades), (1, 3, 0, 0, [0; N_UPGRADES]));
        assert_eq!(w.money, 200.0 + 3.0 * STONE_START_MONEY);
        assert!(w.get(10, 10).is_none() && w.node(10, 10).is_none() && w.get(HUB_X, HUB_Y).is_some());
        assert_eq!((w.catalog_found(), w.great_done, w.rep), (found, true, rep), "каталог, мегапроект и репутация остаются");
        assert!((w.price_mult() - 1.15).abs() < 1e-9 && (w.machine_speed() / speed - 1.06).abs() < 1e-5);
        assert!(w.ach >> 9 & 1 == 1 && w.ach >> 6 & 1 == 1 && w.ach >> 2 & 1 == 1 && w.ach >> 5 & 1 == 1);
        let back = World::load(&w.save()).unwrap();
        assert_eq!((back.cycle, back.stones, back.great_done, back.ach), (1, 3, true, w.ach));
    }

    /// Карта: разломы и преграды есть только в новых играх; старые сохранения видят прежнюю карту.
    #[test]
    fn map_features_only_in_new_games() {
        for seed in [1u64, 11, 1120, 987_654_321] {
            let (old, new) = (World::new(seed), World::new_game(seed));
            assert!(!old.rocks.iter().any(|r| *r) && !old.rich.iter().any(|r| *r));
            // Старое сохранение (без строки M) - карта в точности прежняя.
            let legacy = format!("ALEMBIC1\n100 0 0 0 0 0 0 {}\n10 10 0 1 1\n", seed);
            let w = World::load(&legacy).unwrap();
            assert_eq!((w.veins.clone(), w.map_ver), (old.veins.clone(), 0));
            assert!(!w.rocks.iter().any(|r| *r) && !w.rich.iter().any(|r| *r));
            assert!(!World::load(&w.save()).unwrap().rocks.iter().any(|r| *r), "пересохранение не добавляет преград");
            // Новая игра: преграды не на жилах и не у лавки, разломы - богатые жилы вдали.
            let (rocks, rifts) = (new.rocks.iter().filter(|r| **r).count(), new.rich.iter().filter(|r| **r).count());
            assert!(rocks >= 100 && (18..=30).contains(&rifts), "сид {}: преград {}, клеток разломов {}", seed, rocks, rifts);
            for i in 0..new.veins.len() {
                let d = (i as i32 % W - HUB_X - 1).abs().max((i as i32 / W - HUB_Y - 1).abs());
                assert!(!(new.rocks[i] && (new.veins[i] > 0 || d < 8)));
                assert!(!new.rich[i] || (new.veins[i] > 0 && d >= 25));
                assert!(old.veins[i] == 0 || new.veins[i] > 0, "старые жилы на месте");
            }
            let back = World::load(&new.save()).unwrap();
            assert_eq!((back.rocks.clone(), back.rich.clone(), back.veins.clone()), (new.rocks.clone(), new.rich.clone(), new.veins.clone()));
        }
        // Преграда мешает строить; снос стоит денег и запоминается.
        let mut w = World::new_game(1);
        w.money = 5000.0;
        let i = w.rocks.iter().position(|r| *r).unwrap() as i32;
        let (x, y) = (i % W, i / W);
        assert!(w.place(Kind::Belt, x, y, Dir::E).is_err());
        assert!(w.remove(x, y) && w.money == 5000.0 - ROCK_COST && !w.rocks[i as usize]);
        assert_eq!(w.place(Kind::Belt, x, y, Dir::E), Ok(true));
        let back = World::load(&w.save()).unwrap();
        assert!(!back.rocks[i as usize] && back.get(x, y).is_some());
        assert_eq!(back.rocks.iter().filter(|r| **r).count(), w.rocks.iter().filter(|r| **r).count());
        w.money = 10.0;
        let j = w.rocks.iter().position(|r| *r).unwrap() as i32;
        assert!(!w.remove(j % W, j / W), "без денег преграду не снести");
    }

    // ---------- прохождение с нуля ----------

    /// Дерево рецепта: машина и её входы; лист - добыча стихии.
    struct Plan {
        kind: Kind,
        elem: u8,
        kids: Vec<Plan>,
    }

    fn plan(it: Item) -> Plan {
        let node = |name: &str, kids: Vec<Plan>| Plan { kind: Kind::by_name(name), elem: 0, kids };
        if it.form == 0 {
            if it.purity > 1 {
                return node("ДИСТИЛЛЯТОР", vec![plan(Item::new(it.mask, it.purity - 1, 0))]);
            }
            if it.mask.count_ones() > 1 {
                let a = it.mask & it.mask.wrapping_neg();
                return node("СМЕСИТЕЛЬ", vec![plan(Item::new(a, 1, 0)), plan(Item::new(it.mask & !a, 1, 0))]);
            }
            return Plan { kind: Kind::Extractor, elem: it.mask.trailing_zeros() as u8 + 1, kids: Vec::new() };
        }
        for (i, d) in KINDS.iter().enumerate() {
            match d.role {
                Role::Convert(a, to, _) if to == it.form => {
                    return Plan { kind: Kind(i as u8), elem: 0, kids: vec![plan(Item::new(it.mask, it.purity, a))] }
                }
                Role::Assemble(a, b, to, _) if to == it.form => {
                    let kids = vec![plan(Item::new(it.mask, it.purity, a)), plan(Item::new(it.mask, it.purity, b))];
                    return Plan { kind: Kind(i as u8), elem: 0, kids };
                }
                _ => {}
            }
        }
        panic!("нет рецепта для {}", it.name());
    }

    impl Plan {
        fn width(&self) -> i32 {
            self.kids.iter().map(|k| k.width()).sum::<i32>().max(1)
        }
        fn cost(&self, w: &World) -> f64 {
            w.cost(self.kind) + self.kids.iter().map(|k| k.cost(w) + k.width() as f64).sum::<f64>()
        }
        fn max_unlock(&self) -> usize {
            self.kids.iter().map(|k| k.max_unlock()).max().unwrap_or(0).max(self.kind.unlock_level())
        }
        /// Ставит поддерево: машина в (x, y) выдаёт к магистрали, первый вход - прямо за ней,
        /// второй - из соседней колонки по ленте вбок. `s` - куда растёт модуль (-1 на север).
        fn build(&self, w: &mut World, x: i32, y: i32, s: i32, placed: &mut Vec<(i32, i32)>) {
            let out = if s < 0 { Dir::S } else { Dir::N };
            if self.kids.is_empty() {
                // Бот проверяет экономику, а не карту: жила появляется под его добычей.
                w.veins[(y * W + x) as usize] = self.elem;
            }
            assert_eq!(w.place(self.kind, x, y, out), Ok(true), "{} на ({}, {})", self.kind.name(), x, y);
            placed.push((x, y));
            if let Some(a) = self.kids.first() {
                a.build(w, x, y + s, s, placed);
                if let Some(b) = self.kids.get(1) {
                    let xb = x + a.width();
                    b.build(w, xb, y + s, s, placed);
                    for bx in x + 1..=xb {
                        assert_eq!(w.place(Kind::Belt, bx, y, Dir::W), Ok(true));
                        placed.push((bx, y));
                    }
                }
            }
        }
    }

    /// Новая игра проходится с нуля до финального контракта при включённом насыщении рынка.
    /// Бот играет честно по деньгам и открытиям: $200 на старте, строит только открытое, платит за всё,
    /// улучшений не покупает. Каждому контракту - своя линия; три последние линии продолжают работать.
    #[test]
    fn game_is_beatable_from_scratch() {
        let mut w = World::new(1);
        assert_eq!((w.money, w.level), (200.0, 0));
        for x in 4..HUB_X {
            w.place(Kind::Belt, x, HUB_Y + 1, Dir::E).unwrap();
        }
        // Живые линии: (сторона, x0, ширина, клетки).
        let mut lines: Vec<(i32, i32, i32, Vec<(i32, i32)>)> = Vec::new();
        let mut built = usize::MAX;
        let (mut secs, mut log) = (0u32, Vec::new());
        while w.level < FINAL_LEVEL {
            if built != w.level {
                let p = plan(contract(w.level).item);
                assert!(p.max_unlock() <= w.level, "контракт {} требует ещё закрытую постройку", w.level + 1);
                if w.money >= p.cost(&w) {
                    while lines.len() >= 3 {
                        for (x, y) in lines.remove(0).3 {
                            assert!(w.remove(x, y));
                        }
                    }
                    let width = p.width();
                    // Первое свободное место вдоль магистрали; если линия не влезает - сносим старые.
                    let (s, x0) = loop {
                        let free = |s: i32, x0: i32| lines.iter().all(|l| l.0 != s || x0 + width <= l.1 || l.1 + l.2 <= x0);
                        let spot = [-1, 1].into_iter().flat_map(|s| (4..HUB_X - width).map(move |x| (s, x))).find(|(s, x)| free(*s, *x));
                        match spot {
                            Some(p) => break p,
                            None => {
                                assert!(!lines.is_empty(), "линия шириной {} не влезает вовсе", width);
                                for (x, y) in lines.remove(0).3 {
                                    assert!(w.remove(x, y));
                                }
                            }
                        }
                    };
                    let mut placed = Vec::new();
                    p.build(&mut w, x0, HUB_Y + 1 + s, s, &mut placed);
                    lines.push((s, x0, width, placed));
                    built = w.level;
                    log.push(format!("{}:{}м", w.level + 1, secs / 60));
                }
            }
            for _ in 0..60 {
                w.update(1.0 / 60.0);
            }
            secs += 1;
            assert!(secs < 60 * 60 * 12, "за 12 игровых часов дошли только до контракта {} ({:?})", w.level + 1, log);
        }
        println!("ПРОХОЖДЕНИЕ С НУЛЯ: {} игровых минут до финального контракта, деньги ${:.0}", secs / 60, w.money);
        println!("  линия контракта построена на минуте: {}", log.join(" "));
        assert!(w.ach >> 1 & 1 == 1, "достижение за финальный контракт");
    }

    // ---------- слой сигналов ----------

    fn lg(w: &mut World, name: &str, x: i32, y: i32, d: Dir) {
        assert_eq!(w.place(Kind::by_name(name), x, y, d), Ok(true), "{} на ({}, {})", name, x, y);
    }

    fn ticks(w: &mut World, n: u32) {
        for _ in 0..n {
            w.update(1.0 / LOGIC_HZ);
        }
    }

    fn out(w: &World, x: i32, y: i32) -> bool {
        w.node(x, y).expect("деталь на месте").out
    }

    /// Постоянный сигнал: вентиль НЕ без входов всегда включён.
    fn lever(w: &mut World, x: i32, y: i32, d: Dir, on: bool) {
        w.force_clear_logic(x, y);
        if on {
            lg(w, "ВЕНТИЛЬ НЕ", x, y, d);
        }
    }

    /// Вентили: И, ИЛИ, НЕ по таблице истинности; защёлка помнит; счётчик считает фронты.
    #[test]
    fn logic_gates() {
        for (gate, table) in [("ВЕНТИЛЬ И", [false, false, false, true]), ("ВЕНТИЛЬ ИЛИ", [false, true, true, true]), ("ВЕНТИЛЬ НЕ", [true, false, false, false])] {
            for (k, want) in table.iter().enumerate() {
                let mut w = world();
                // Входы: рычаг сзади (через провод) и рычаг сбоку (вплотную).
                lg(&mut w, gate, 12, 10, Dir::E);
                lg(&mut w, "ПРОВОД", 11, 10, Dir::N);
                lg(&mut w, "ПРОВОД", 12, 11, Dir::N);
                lever(&mut w, 10, 10, Dir::E, k & 1 == 1);
                lever(&mut w, 12, 12, Dir::N, k & 2 == 2);
                ticks(&mut w, 4);
                assert_eq!(out(&w, 12, 10), *want, "{} при входах {:02b}", gate, k);
            }
        }
        // У вентиля И неподключённый вход не мешает: один подключённый включённый вход - выход включён.
        let mut w = world();
        lg(&mut w, "ВЕНТИЛЬ И", 12, 10, Dir::E);
        lever(&mut w, 11, 10, Dir::E, true);
        ticks(&mut w, 3);
        assert!(out(&w, 12, 10));
        // Провод несёт сигнал по всей связной цепочке, но не дальше разрыва.
        for x in 13..20 {
            lg(&mut w, "ПРОВОД", x, 10, Dir::N);
        }
        lg(&mut w, "ПРОВОД", 21, 10, Dir::N);
        ticks(&mut w, 2);
        assert!(w.powered[(10 * W + 19) as usize] && !w.powered[(10 * W + 21) as usize]);

        // Защёлка: вход сзади включает, вход сбоку сбрасывает, без входов состояние держится.
        let mut w = world();
        lg(&mut w, "ЗАЩЁЛКА", 12, 10, Dir::E);
        ticks(&mut w, 3);
        assert!(!out(&w, 12, 10));
        lever(&mut w, 11, 10, Dir::E, true);
        ticks(&mut w, 3);
        lever(&mut w, 11, 10, Dir::E, false);
        ticks(&mut w, 10);
        assert!(out(&w, 12, 10), "помнит включение");
        lever(&mut w, 12, 11, Dir::N, true);
        ticks(&mut w, 3);
        lever(&mut w, 12, 11, Dir::N, false);
        ticks(&mut w, 10);
        assert!(!out(&w, 12, 10), "помнит сброс");

        // Счётчик до 3: считает фронты, на третьем даёт импульс и начинает заново; сброс сбоку.
        let mut w = world();
        lg(&mut w, "СЧЁТЧИК", 12, 10, Dir::E);
        assert!(w.set_lparam(12, 10, 3));
        let mut pulses = 0;
        for k in 0..7 {
            lever(&mut w, 11, 10, Dir::E, true);
            for _ in 0..3 {
                ticks(&mut w, 1);
                pulses += out(&w, 12, 10) as u32;
            }
            lever(&mut w, 11, 10, Dir::E, false);
            ticks(&mut w, 3);
            assert_eq!((w.node(12, 10).unwrap().count, pulses), ((k + 1) % 3, (k + 1) / 3), "после {} импульсов", k + 1);
        }
        lever(&mut w, 12, 11, Dir::N, true);
        ticks(&mut w, 3);
        assert_eq!(w.node(12, 10).unwrap().count, 0, "сброс");
    }

    /// Схема из трёх деталей: таймер -> (И + НЕ = выделитель фронта) -> счётчик до 3.
    /// Строится в точке (ox, oy); `flip` разворачивает её на 180 градусов.
    fn edge_circuit(w: &mut World, ox: i32, oy: i32, flip: bool) -> [(i32, i32); 2] {
        let s = if flip { -1 } else { 1 };
        let d = |d: Dir| if flip { d.opp() } else { d };
        let at = |dx: i32, dy: i32| (ox + s * dx, oy + s * dy);
        let mut put = |name: &str, dx: i32, dy: i32, dir: Dir| {
            let (x, y) = at(dx, dy);
            lg(w, name, x, y, d(dir));
        };
        put("ТАЙМЕР", 0, 0, Dir::E);
        put("ПРОВОД", 1, 0, Dir::N);
        put("ПРОВОД", 2, 0, Dir::N);
        put("ВЕНТИЛЬ НЕ", 2, 1, Dir::E);
        put("ПРОВОД", 3, 1, Dir::N);
        put("ВЕНТИЛЬ И", 3, 0, Dir::E);
        put("СЧЁТЧИК", 4, 0, Dir::E);
        let (t, c) = (at(0, 0), at(4, 0));
        assert!(w.set_lparam(t.0, t.1, 2) && w.set_lparam(c.0, c.1, 3));
        [at(3, 0), c]
    }

    /// Схема из трёх вентилей даёт ожидаемую последовательность - и одну и ту же при любом
    /// порядке обхода тайлов (развёрнутая схема) и любой частоте кадров.
    #[test]
    fn three_gate_circuit_sequence() {
        let mut seqs = Vec::new();
        for flip in [false, true] {
            let mut w = world();
            let [and, counter] = edge_circuit(&mut w, 30, 30, flip);
            let (mut a, mut c) = (Vec::new(), Vec::new());
            for t in 1..=70 {
                ticks(&mut w, 1);
                if out(&w, and.0, and.1) {
                    a.push(t);
                }
                if out(&w, counter.0, counter.1) {
                    c.push(t);
                }
            }
            // Таймер 0.2 с = 12 тактов; И даёт импульс в такт через такт после фронта; счётчик - каждый третий.
            assert_eq!(a, vec![2, 14, 26, 38, 50, 62], "импульсы выделителя фронта (flip={})", flip);
            assert_eq!(c, vec![27, 63], "импульсы счётчика (flip={})", flip);
            seqs.push((a, c));
        }
        assert_eq!(seqs[0], seqs[1], "порядок обхода тайлов не влияет");
        // Частота кадров не влияет: после 120 тактов состояние одно и то же.
        let mut states = Vec::new();
        for fps in FPS {
            let mut w = world();
            edge_circuit(&mut w, 30, 30, false);
            while w.logic_ticks < 120 {
                w.update(1.0 / fps as f32);
            }
            if w.logic_ticks == 120 {
                states.push(w.logic_state());
            } else {
                // На 30 к/с такты идут парами - 120 достигается ровно.
                panic!("{} к/с: перескочили такт 120", fps);
            }
        }
        assert!(states.windows(2).all(|p| p[0] == p[1]));
        // Схема переживает сохранение вместе с настройками деталей.
        let mut w = world();
        edge_circuit(&mut w, 30, 30, false);
        let saved = w.save();
        assert!(saved.contains(&format!("L 34 30 {} 1 3\n", Kind::by_name("СЧЁТЧИК").0)), "{}", saved);
        let back = World::load(&saved).unwrap();
        assert_eq!(back.save(), saved);
        assert_eq!(back.node(30, 30).map(|n| (n.lk(), n.param)), Some((Lk::Timer, 2)));
        assert_eq!(back.logic.iter().flatten().count(), 7);
        // Детали логики клеток не занимают: под ними можно строить, снос постройки их не трогает.
        let mut w = back;
        w.money = 1e6;
        w.place(Kind::Belt, 30, 30, Dir::E).unwrap();
        assert!(w.remove(30, 30) && w.node(30, 30).is_some());
        let money = w.money;
        assert!(w.remove_logic(30, 30) && w.node(30, 30).is_none());
        assert_eq!(w.money, money + 200.0, "цена таймера возвращается");
    }

    /// Память деталей (защёлка, счётчик) переживает сохранение; копия мира в сети своим лентам
    /// не верит и импульсы датчика предмета сама не считает.
    #[test]
    fn logic_memory_is_saved_and_replica_trusts_host() {
        let k = Kind::by_name;
        let mut w = World::new(1);
        w.money = 1e6;
        w.level = 30;
        w.place(k("ТАЙМЕР"), 9, 10, Dir::E).unwrap();
        w.place(k("ЗАЩЁЛКА"), 10, 10, Dir::E).unwrap();
        w.place(Kind(0), 20, 20, Dir::E).unwrap();
        w.place(k("ДАТЧИК ПРЕДМЕТА"), 20, 20, Dir::N).unwrap();
        (0..10).for_each(|_| w.update(1.0 / 60.0));
        assert!(w.node(10, 10).unwrap().out, "таймер включил защёлку");
        assert!(w.remove_logic(9, 10));
        (0..10).for_each(|_| w.update(1.0 / 60.0));
        assert!(w.node(10, 10).unwrap().out, "защёлка помнит состояние без входа");

        let saved = w.save();
        assert!(saved.contains("LS 10:10:1:0:0\n"), "{}", saved);
        let back = World::load(&saved).expect("сохранение читается");
        assert_eq!(back.logic_state(), w.logic_state());
        // Сохранение без строк памяти (как до этого обновления) читается, детали в нём выключены.
        let old: String = saved.lines().filter(|l| !l.starts_with("LS ")).map(|l| format!("{}\n", l)).collect();
        let back = World::load(&old).expect("старый вид читается");
        assert!(back.node(10, 10).is_some() && !back.node(10, 10).unwrap().out);
        // Мусор в строке памяти не мешает загрузке.
        assert!(World::load(&format!("{}LS 999:1:1\nLS x\nLS 10:10:1:7:1:5\n", saved)).is_some());

        // Хост считает въехавший предмет и просит разослать логику; копия мира - нет.
        w.logic_dirty = false;
        assert!(w.try_insert(20, 20, Item::base(0), Dir::E, 0.0));
        assert!(w.node(20, 20).unwrap().count == 1 && w.logic_dirty);
        let mut rep = World::load(&saved).unwrap();
        rep.replica = true;
        assert!(rep.try_insert(20, 20, Item::base(0), Dir::E, 0.0));
        assert!(rep.node(20, 20).unwrap().count == 0 && !rep.logic_dirty);
        // Но присланный хостом счёт копия отыгрывает сама: импульс уходит дальше по схеме.
        rep.apply_logic_state(w.logic_ticks, &w.logic_state());
        assert_eq!(rep.node(20, 20).unwrap().count, 1);
        rep.update(1.0 / 60.0);
        assert!(rep.node(20, 20).unwrap().out && rep.node(20, 20).unwrap().count == 0);
    }

    /// Датчики: затор, заполнение склада, предмет по условию, таймер.
    #[test]
    fn logic_sensors() {
        // Затор: лента упирается в тупик.
        let mut w = world();
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(Kind::Belt, 11, 10, Dir::E).unwrap();
        lg(&mut w, "ДАТЧИК ЗАТОРА", 11, 10, Dir::S);
        run(&mut w, 1.0);
        assert!(!out(&w, 11, 10), "пока лента не забита, сигнала нет");
        run(&mut w, 8.0);
        assert!(out(&w, 11, 10));
        w.place(Kind::by_name("УТИЛИЗАТОР"), 12, 10, Dir::N).unwrap();
        run(&mut w, 2.0);
        assert!(!out(&w, 11, 10), "затор ушёл");

        // Склад: порог в процентах от вместимости (100 мест без улучшений).
        let mut w = world();
        source(&mut w, 10, 10, Dir::E, Item::base(0), 40);
        w.set_ports(10, 10, pack_ports([PORT_OFF, PORT_OFF, PORT_IN, PORT_OFF]));
        lg(&mut w, "ДАТЧИК СКЛАДА", 10, 10, Dir::S);
        assert!(w.set_lparam(10, 10, 40));
        ticks(&mut w, 2);
        assert!(!out(&w, 10, 10));
        assert!(w.try_insert(10, 10, Item::base(0), Dir::E, 0.0));
        ticks(&mut w, 2);
        assert!(out(&w, 10, 10), "40 из 100 при пороге 40%");

        // Предмет по условию: импульс на каждый подходящий, счёт не теряется на любой частоте кадров.
        for fps in FPS {
            let mut w = world();
            w.veins[(10 * W + 10) as usize] = 1;
            w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
            sink(&mut w, 11, 10, Dir::E);
            lg(&mut w, "ДАТЧИК ПРЕДМЕТА", 11, 10, Dir::S);
            lg(&mut w, "СЧЁТЧИК", 11, 11, Dir::S);
            lg(&mut w, "СЧЁТЧИК", 11, 12, Dir::S);
            assert!(w.set_lparam(11, 11, 4) && w.set_lparam(11, 12, 9999));
            run_fps(&mut w, 60.0, fps);
            let n = stored(&w, 11, 10);
            assert!(n >= 45, "{} к/с: {}", fps, n);
            // Ни один предмет не потерян: всё либо досчитано, либо ещё идёт по цепочке импульсом.
            let (s, c4, big) = (*w.node(11, 10).unwrap(), *w.node(11, 11).unwrap(), *w.node(11, 12).unwrap());
            let seen = big.count * 4 + c4.out as u32 * 4 + c4.count + s.count + s.out as u32;
            assert_eq!(seen, n, "{} к/с: учтено {} из {}", fps, seen, n);
            assert!(big.count >= n / 4 - 1, "{} к/с: каждый четвёртый из {}", fps, n);
            // Условие: вода датчик огня не трогает.
            assert!(w.set_lparam(11, 10, Cond { mask: 2, ..Cond::default() }.bits()));
            ticks(&mut w, 8);
            let before = (w.node(11, 11).unwrap().count, w.node(11, 12).unwrap().count);
            run_fps(&mut w, 10.0, fps);
            assert_eq!((w.node(11, 11).unwrap().count, w.node(11, 12).unwrap().count), before, "{} к/с", fps);
        }

        // Таймер: половину периода включён, половину выключен; период в десятых долях секунды.
        let mut w = world();
        lg(&mut w, "ТАЙМЕР", 10, 10, Dir::E);
        assert!(w.set_lparam(10, 10, 10));
        let mut on = 0;
        for _ in 0..300 {
            ticks(&mut w, 1);
            on += out(&w, 10, 10) as u32;
        }
        assert_eq!(on, 150, "5 секунд при периоде 1 с");
        assert_eq!((clean_lparam(Lk::Timer, 0), clean_lparam(Lk::SenStore, 500), clean_lparam(Lk::Act, 7)), (2, 100, 1));
    }

    /// Исполнители: ворота на ленте, останов машины, выбор выхода переключателя.
    #[test]
    fn logic_actuators() {
        for fps in FPS {
            // Ворота: исполнитель на ленте, сигнал останавливает.
            let mut w = world();
            w.veins[(10 * W + 10) as usize] = 1;
            w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
            w.place(Kind::Belt, 11, 10, Dir::E).unwrap();
            w.place(Kind::Belt, 12, 10, Dir::E).unwrap();
            sink(&mut w, 13, 10, Dir::E);
            lg(&mut w, "ИСПОЛНИТЕЛЬ", 12, 10, Dir::N);
            lever(&mut w, 12, 11, Dir::N, true);
            run_fps(&mut w, 20.0, fps);
            assert_eq!(stored(&w, 13, 10), 0, "{} к/с: ворота закрыты", fps);
            assert!(w.is_halted(12, 10) && !lane_items(&w, 12, 10).is_empty() && lane_items(&w, 11, 10).len() == 2);
            lever(&mut w, 12, 11, Dir::N, false);
            run_fps(&mut w, 10.0, fps);
            assert!(stored(&w, 13, 10) >= 8 && !w.is_halted(12, 10), "{} к/с: ворота открыты", fps);
            // Режим "сигнал включает": без сигнала стоит.
            assert!(w.set_lparam(12, 10, 1));
            let n = stored(&w, 13, 10);
            run_fps(&mut w, 5.0, fps);
            assert!(stored(&w, 13, 10) <= n + 1 && w.is_halted(12, 10), "{} к/с", fps);
            lever(&mut w, 12, 11, Dir::N, true);
            run_fps(&mut w, 5.0, fps);
            assert!(stored(&w, 13, 10) > n + 2, "{} к/с", fps);
        }
        // Машина: остановленный кристаллизатор не работает и не выдаёт.
        let mut w = bench("КРИСТАЛЛИЗАТОР");
        lg(&mut w, "ИСПОЛНИТЕЛЬ", 11, 10, Dir::N);
        lever(&mut w, 11, 9, Dir::S, true);
        ticks(&mut w, 3);
        assert!(feed(&mut w, 11, 10, Item::base(0)));
        run(&mut w, 6.0);
        assert!(lane_items(&w, 12, 10).is_empty() && w.get(11, 10).unwrap().progress == 0.0);
        lever(&mut w, 11, 9, Dir::S, false);
        run(&mut w, 3.0);
        assert_eq!(lane_items(&w, 12, 10), vec![Item::new(1, 1, 1)]);
        // Переключатель: сигнал выбирает второй выход вместо таймера.
        let mut w = world();
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(Kind::by_name("ПЕРЕКЛЮЧАТЕЛЬ"), 11, 10, Dir::E).unwrap();
        w.set_ports(11, 10, pack_ports([PORT_OUT, PORT_OUT, PORT_IN, PORT_OFF]));
        sink(&mut w, 12, 10, Dir::E);
        sink(&mut w, 11, 11, Dir::S);
        lg(&mut w, "ИСПОЛНИТЕЛЬ", 11, 10, Dir::N);
        run(&mut w, 50.0);
        // Без сигнала - всегда первый выход, хотя по таймеру (20 с) он бы уже менялся.
        assert!(stored(&w, 12, 10) >= 35 && stored(&w, 11, 11) == 0);
        lever(&mut w, 11, 9, Dir::S, true);
        let first = stored(&w, 12, 10);
        run(&mut w, 20.0);
        assert!(stored(&w, 11, 11) >= 14 && stored(&w, 12, 10) <= first + 1);
        assert!(!w.is_halted(11, 10), "переключатель не останавливается");
        // Снятый исполнитель отпускает постройку.
        let mut w = bench("КРИСТАЛЛИЗАТОР");
        lg(&mut w, "ИСПОЛНИТЕЛЬ", 11, 10, Dir::N);
        lever(&mut w, 11, 9, Dir::S, true);
        ticks(&mut w, 3);
        assert!(w.is_halted(11, 10));
        assert!(w.remove_logic(11, 10) && !w.is_halted(11, 10));
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

    /// Переключатель пускает всё в один выход и меняет его по таймеру.
    #[test]
    fn switch_alternates_by_timer() {
        let mut w = world();
        let sw = Kind::by_name("ПЕРЕКЛЮЧАТЕЛЬ");
        w.veins[(10 * W + 10) as usize] = 1;
        w.place(Kind::Extractor, 10, 10, Dir::E).unwrap();
        w.place(sw, 11, 10, Dir::E).unwrap();
        // Вперёд - склад, вправо (на юг) - торговый пост, влево закрыто.
        w.set_ports(11, 10, pack_ports([PORT_OUT, PORT_OUT, PORT_IN, PORT_OFF]));
        w.place(Kind::by_name("СКЛАД"), 12, 10, Dir::E).unwrap();
        w.set_ports(12, 10, pack_ports([PORT_OFF, PORT_OFF, PORT_IN, PORT_OFF]));
        w.place(Kind::by_name("ТОРГОВЫЙ ПОСТ"), 11, 11, Dir::N).unwrap();
        run(&mut w, 18.0);
        let stored = w.get(12, 10).unwrap().count;
        assert!(stored >= 5 && w.total_sold == 0, "первые 20 с всё идёт вперёд: {} {}", stored, w.total_sold);
        run(&mut w, 18.0);
        assert!(w.total_sold >= 5, "после переключения всё идёт вбок: {}", w.total_sold);
        assert!(w.get(12, 10).unwrap().count <= stored + 2);
        // Период задаётся уровнем и переживает сохранение.
        w.force_level(11, 10, 3);
        let back = World::load(&w.save()).unwrap();
        assert_eq!(back.get(11, 10).unwrap().level, 3);
        assert_eq!(switch_period(3), 60.0);
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
