//! Japanese conversion model interfaces: a reading dictionary with Mozc-style
//! costs and part-of-speech ids, and the POS connection-cost matrix. The full
//! model is an external asset (D15, `ja_asset::JaAsset`); `SampleDictionary`
//! is a tiny stand-in so the engine and its tests run without it.

/// How an entry's surface is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface<'a> {
    Text(&'a str),
    /// Same as the matched reading (hiragana words, particles).
    Reading,
    /// Katakana of the matched reading.
    Katakana,
}

/// One dictionary word. `lid`/`rid` are Mozc POS ids (left/right context);
/// lower cost is more likely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub surface: Surface<'a>,
    pub lid: u16,
    pub rid: u16,
    pub cost: i16,
}

pub trait Dictionary {
    /// Calls `f(byte_len, entry)` for every entry whose reading is a prefix of
    /// `text` (hiragana). Entries borrow from `self`.
    fn common_prefix<'a>(&'a self, text: &str, f: &mut dyn FnMut(usize, Entry<'a>));
    /// POS id given to unknown-word fallback nodes (a common noun).
    fn unknown_id(&self) -> u16;
}

pub trait Connection {
    /// Cost of a word with right id `rid` followed by one with left id `lid`.
    fn cost(&self, rid: u16, lid: u16) -> i32;
    /// POS id of the sentence start/end markers.
    fn bos_eos(&self) -> u16 {
        0
    }
}

/// STAND-IN: a tiny hand-written sample so conversion works end to end
/// without the asset. Costs and POS ids are invented, not Mozc's.
pub struct SampleDictionary;

const BOS_EOS: u16 = 0;
const NOUN: u16 = 1;
const PARTICLE: u16 = 2;
const VERB: u16 = 3;
const AUX: u16 = 4;
const POS_COUNT: usize = 5;

/// `CONNECTION[rid][lid]`.
const CONNECTION: [[i16; POS_COUNT]; POS_COUNT] = [
    //  EOS   NOUN  PART  VERB  AUX
    [0, 500, 3000, 800, 3000],  // BOS
    [200, 1500, 200, 1200, 300], // NOUN
    [300, 300, 2500, 400, 2000], // PARTICLE
    [300, 1500, 900, 2000, 500], // VERB
    [100, 1000, 600, 1500, 1200], // AUX
];

const SAMPLE: &[(&str, &str, u16, i16)] = &[
    ("さけ", "酒", NOUN, 3000),
    ("さけ", "鮭", NOUN, 3200),
    ("にほん", "日本", NOUN, 2500),
    ("にほんご", "日本語", NOUN, 2600),
    ("にっぽん", "日本", NOUN, 3500),
    ("こんにちは", "今日は", NOUN, 3000),
    ("こんばんは", "今晩は", NOUN, 3000),
    ("ありがとう", "有難う", NOUN, 3000),
    ("わたし", "私", NOUN, 2000),
    ("あなた", "貴方", NOUN, 2500),
    ("ひと", "人", NOUN, 2500),
    ("やま", "山", NOUN, 2800),
    ("かわ", "川", NOUN, 2800),
    ("かわ", "河", NOUN, 3300),
    ("みず", "水", NOUN, 2800),
    ("ひ", "日", NOUN, 3500),
    ("ひ", "火", NOUN, 3600),
    ("つき", "月", NOUN, 3000),
    ("き", "木", NOUN, 3500),
    ("き", "気", NOUN, 3600),
    ("かね", "金", NOUN, 3000),
    ("つち", "土", NOUN, 3200),
    ("そら", "空", NOUN, 3000),
    ("うみ", "海", NOUN, 3000),
    ("あめ", "雨", NOUN, 3000),
    ("あめ", "飴", NOUN, 3500),
    ("ゆき", "雪", NOUN, 3000),
    ("はな", "花", NOUN, 3000),
    ("はな", "鼻", NOUN, 3300),
    ("いぬ", "犬", NOUN, 3000),
    ("ねこ", "猫", NOUN, 3000),
    ("とり", "鳥", NOUN, 3000),
    ("さかな", "魚", NOUN, 3000),
    ("くるま", "車", NOUN, 3000),
    ("でんしゃ", "電車", NOUN, 3000),
    ("えき", "駅", NOUN, 3000),
    ("がっこう", "学校", NOUN, 2800),
    ("せんせい", "先生", NOUN, 2800),
    ("がくせい", "学生", NOUN, 2800),
    ("ともだち", "友達", NOUN, 2800),
    ("かぞく", "家族", NOUN, 3000),
    ("いえ", "家", NOUN, 3000),
    ("みせ", "店", NOUN, 3000),
    ("ほん", "本", NOUN, 2800),
    ("じかん", "時間", NOUN, 2800),
    ("きょう", "今日", NOUN, 2500),
    ("あした", "明日", NOUN, 2500),
    ("きのう", "昨日", NOUN, 2500),
    ("いま", "今", NOUN, 2500),
    ("なまえ", "名前", NOUN, 2800),
    ("でんわ", "電話", NOUN, 2800),
    ("てがみ", "手紙", NOUN, 3000),
    ("きって", "切手", NOUN, 3000),
    ("しごと", "仕事", NOUN, 2800),
    ("かいしゃ", "会社", NOUN, 2800),
    ("せかい", "世界", NOUN, 2800),
    ("にんげん", "人間", NOUN, 2800),
    ("おとこ", "男", NOUN, 3000),
    ("おんな", "女", NOUN, 3000),
    ("こども", "子供", NOUN, 3000),
    ("め", "目", NOUN, 3500),
    ("て", "手", NOUN, 3500),
    ("あし", "足", NOUN, 3200),
    ("くち", "口", NOUN, 3200),
    ("みみ", "耳", NOUN, 3200),
    ("あたま", "頭", NOUN, 3000),
    ("こころ", "心", NOUN, 3000),
    ("かんじ", "漢字", NOUN, 2800),
    ("かんじ", "感じ", NOUN, 3000),
    ("にゅうりょく", "入力", NOUN, 2800),
    ("へんかん", "変換", NOUN, 2800),
    ("にほんじん", "日本人", NOUN, 2700),
    ("かんこく", "韓国", NOUN, 2800),
    ("かんこくご", "韓国語", NOUN, 2900),
    ("えいご", "英語", NOUN, 2800),
    ("たべる", "食べる", VERB, 3000),
    ("のむ", "飲む", VERB, 3000),
    ("みる", "見る", VERB, 3000),
    ("いく", "行く", VERB, 3000),
    ("くる", "来る", VERB, 3000),
    ("かく", "書く", VERB, 3000),
    ("よむ", "読む", VERB, 3000),
    ("はなす", "話す", VERB, 3000),
    ("おおきい", "大きい", VERB, 3000),
    ("ちいさい", "小さい", VERB, 3000),
    ("あたらしい", "新しい", VERB, 3000),
    ("たかい", "高い", VERB, 3000),
    ("は", "は", PARTICLE, 1000),
    ("が", "が", PARTICLE, 1000),
    ("を", "を", PARTICLE, 1000),
    ("の", "の", PARTICLE, 1000),
    ("に", "に", PARTICLE, 1000),
    ("で", "で", PARTICLE, 1200),
    ("と", "と", PARTICLE, 1200),
    ("も", "も", PARTICLE, 1200),
    ("です", "です", AUX, 1000),
    ("ます", "ます", AUX, 1200),
];

impl Dictionary for SampleDictionary {
    fn common_prefix<'a>(&'a self, text: &str, f: &mut dyn FnMut(usize, Entry<'a>)) {
        for &(reading, surface, pos, cost) in SAMPLE {
            if text.starts_with(reading) {
                let surface = if surface == reading { Surface::Reading } else { Surface::Text(surface) };
                f(reading.len(), Entry { surface, lid: pos, rid: pos, cost });
            }
        }
    }

    fn unknown_id(&self) -> u16 {
        NOUN
    }
}

impl Connection for SampleDictionary {
    fn cost(&self, rid: u16, lid: u16) -> i32 {
        CONNECTION
            .get(rid as usize)
            .and_then(|row| row.get(lid as usize))
            .map_or(i16::MAX as i32, |&c| c as i32)
    }

    fn bos_eos(&self) -> u16 {
        BOS_EOS
    }
}
