# NullIME 구조

스펙은 `docs/spec.md`에 있다. 이 문서는 엔진과 플랫폼 구현 사이의 경계를 정한다.

## 디렉터리

```
Cargo.toml                 workspace
crates/nullime-engine      순수 Rust 엔진. 플랫폼 코드 없음.
crates/nullime-wasm        wasm-bindgen 바인딩. 웹 데모가 쓴다.
web/                       웹 데모. 정적 HTML + wasm-bindgen 산출물
platform/windows           TSF 텍스트 서비스 DLL (Rust, `windows` 크레이트). 엔진 크레이트를 직접 쓴다.
tools/nullime-dict         일본어 사전 애셋 변환기와 정확도·지연 측정 도구. 배포물에 들어가지 않는다.
```

macOS(InputMethodKit 입력기)는 지금 만들지 않는다 (D14). 나중에 UniFFI 바인딩으로 이식한다.

## 키 모델

플랫폼은 물리 키를 US QWERTY 기준 문자로 바꿔서 엔진에 넘긴다. OS 자판 배열과 무관하게 같은 키는 같은 값이 된다.

```rust
pub enum Key { Char(u8), Space, Enter, Backspace, Left, Right, Up, Down, Other }
pub struct KeyEvent { pub key: Key, pub shift: bool, pub modifier: bool /* Ctrl/Alt/Cmd */ }
```

`Char`는 US QWERTY의 인쇄 가능 ASCII(0x21..=0x7E)다. 영문자는 소문자로 넘기고 Shift는 `shift`로 전달한다 (두벌식 쌍자음 때문). 숫자·문장부호는 Shift가 적용된 결과 문자로 넘긴다 (예: Shift+1 → `!`). 그 밖의 키는 `Key::Other`로 넘긴다.

숫자·문장부호도 키 입력열에 들어간다 (D13). 모든 언어 해석은 그 문자를 그대로 출력한다.

## 엔진 응답

```rust
pub struct Response {
    pub handled: bool,      // false면 플랫폼이 원래 키를 앱에 그대로 넘긴다
    pub commit: String,     // 앱에 삽입할 확정 텍스트 (비어 있을 수 있음)
    pub preedit: Option<Preedit>, // None이면 조합 창을 닫는다
}
pub struct Preedit {
    pub segments: Vec<Segment>, // 칸 목록. 이어 붙이면 조합 창 텍스트
    pub focused: usize,         // 좌우 화살표로 움직이는 칸
}
pub struct Segment {
    pub text: String,
    pub lang: Lang,                 // En | Ko | Ja
    pub candidates: Vec<Candidate>, // 상하 화살표 대상
    pub selected: usize,
}
pub struct Candidate {
    pub text: String,
    pub lang: Lang,
    pub cost: Option<i32>, // 일본어 변환 비용(낮을수록 그럴듯함). 그 밖의 후보는 None
}
```

serde로 내보낼 때 `cost`가 `None`이면 필드를 생략한다.

키별 동작:

- `Char`: 키 입력열에 추가하고 다시 해석한다 (D4). `handled = true`.
- `Space`, `Enter`: 조합 중이면 각 칸의 선택 후보를 이어 붙여 `commit`으로 내고 `handled = false` (D5: 키 자체는 앱이 받는다). 조합 중이 아니면 `handled = false`.
- `Backspace`: 조합 중이면 마지막 키 입력을 취소한다 (D9). 비면 조합을 끝낸다. 조합 중이 아니면 `handled = false` (D10).
- `Left`/`Right`: 조합 중이면 `focused` 이동 (D8). 아니면 `handled = false`.
- `Up`/`Down`: 조합 중이면 포커스된 칸의 `selected` 이동 (D8). 아니면 `handled = false`.
- `Other` 또는 `modifier = true`: 조합 중이면 확정 텍스트를 내고 `handled = false`.

구현에서 정한 세부 (엔진 구현 시 추가):

- `Char`로 대문자 `A`-`Z`가 오면 소문자 + `shift = true`로 바꿔 받는다.
- `Left`/`Right`는 양 끝에서 멈춘다. 칸이 하나여도 조합 중이면 `handled = true`다 (앱 커서가 움직이지 않게).
- `Up`/`Down`은 후보 목록의 끝에서 반대쪽 끝으로 돌아간다. `Down`이 다음 후보다.
- 백스페이스로 조합이 비면 `Response { handled: true, commit: "", preedit: None }`이다.
- `reset()`의 응답은 `handled = false`이고 `commit`에 확정 텍스트를 담는다. 조합 중이 아니면 `commit`은 빈 문자열이다.
- 키 입력 뒤에는 `focused`가 마지막 칸으로 간다.

## 해석 (lattice)

- 키 입력열 `keys[0..n]`의 모든 구간 `keys[i..j]`에 대해 각 `Interpreter`(영어, 두벌식, Romaji+한자)가 후보를 낸다.
- `Scorer` 트레이트가 후보 점수를 매긴다 (D7: 교체 가능). MVP scorer `BaselineScorer`는 아래 "점수"의 언어 모델을 쓴다.
- 동적 계획법으로 최고 점수 경로를 고른다. 그 경로의 구간들이 칸이 된다. 칸이 늘수록 벌점을 준다.
- 칸 경계가 한 단어 안(영문자 키와 영문자 키 사이)에 떨어지면, 앞뒤 칸의 언어에 따라 벌점을 준다 (`Scorer::mid_word_switch_penalty`).
  - 이 벌점이 없으면 단어 목록에 없는 영어 단어가 따로따로 점수가 높은 한글·가나 조각으로 쪼개진다 (`Signing` → `냐후|in|g`).
  - 벌점이 앞 칸의 언어에 달려 있으므로, 위치마다 최고 경로 하나가 아니라 마지막 칸의 언어별로 최고 경로를 하나씩 둔다.
  - `BaselineScorer`는 한국어 → 한국어, 일본어 → 일본어를 벌점 없이 둔다. 같은 문자 체계 안에서는 끊는 위치에 따라 음절 조합이 달라지기 때문이다. 영어 뒤 한국어(`Rustfmf` → `Rust를`)와 일본어(`Rustwo` → `Rust|を`)는 조사로 보고 그 칸을 다시 매긴다. 한국어 칸의 확률을 `0.999 × P_조사(칸) + 0.001 × P_음절(칸)`으로 바꾸고, 벌점은 그 로그 차이다 (아래 "점수"). 조사가 아닌 한국어는 ln 0.001 ≈ -6.9를 더 치른다. 이것이 없으면 흔한 음절이 약한 영어 단어에서 떨어져 나가고(`http` → `ht|세`), 영어처럼 보이는 앞부분이 한국어 단어에서 떨어져 나간다(`Ehaqkdrkrgk` → `Eha|방각하`). 영어 뒤 일본어도 같은 꼴이다. 조사 모델은 조사 14개(`は が を に へ と で も の から まで より や か`)에 고르게 나눈 확률이다. 조사 칸의 점수는 `ln(0.999 / 14 + 0.001 × P_가나(칸))` + 키 수 × ln 26 하나뿐이고, 일본어 칸의 -6과 사전 증거는 받지 않는다. 그래서 조사 점수가 사전의 조사 비용에 달리지 않는다 (`の`의 경로 비용: 표본 사전 4300, Mozc 1601). 대신 벌점 5를 더 받는다. 4.9보다 작으면 `node`가 `no|で`가 되고, 5.4보다 크면 `GitHubni`가 영어로 남는다. 조사가 아닌 일본어의 벌점은 -ln 0.001 + 5 ≈ 11.9다. 영어 → 영어(`Testing` → `Test|in|g`)를 포함한 나머지는 벌점 5를 준다.
- 각 칸의 후보 목록은 같은 구간에 대한 모든 언어의 후보를 담는다. 맨 앞은 경로가 고른 후보다. 언어 전환 벌점 때문에 이 후보가 구간 안에서 점수가 가장 높지 않을 수 있다. 나머지는 점수순이다.
- 사용자가 상하 화살표로 고른 칸은 고정된다. 이후 키 입력은 마지막 고정 칸 뒤 구간만 다시 해석한다. 백스페이스가 고정 칸 안으로 들어가면 그 칸의 고정을 푼다.
- 마지막 고정 칸 앞에 있는 고정되지 않은 칸도 다시 해석하지 않는다. 마지막 고정 칸까지가 통째로 굳는다.
- 한 칸은 최대 32 키 입력이다 (`decode::MAX_SPAN`).
- 해석은 점진적이다 (`decode::Lattice`). 위치 j까지의 최고 경로는 `keys[..j]`에만 달려 있으므로, 키 하나를 더하면 그 키에서 끝나는 구간(최대 32개)만 계산하고 백스페이스는 마지막 노드를 버린다. 고정 칸이 바뀌어 다시 해석할 시작점이 달라지면 그 뒤를 새로 계산한다. 결과는 매번 처음부터 해석한 것과 같다.
- 어떤 해석기도 읽지 못한 구간은 키를 입력한 그대로(`En`) 낮은 점수(키당 -10)의 후보 하나로 둔다. 그래서 모든 칸은 후보가 하나 이상이다.
- 교체 지점: `Interpreter`(구간 → 후보, 언어별 선호 순서), `Scorer`(`score(strokes, candidate, rank)`는 높을수록 좋은 점수, `segment_penalty()`와 `mid_word_switch_penalty(prev, strokes, next)`는 경로 점수에서 빼는 벌점이며 뒤의 것은 단어 안 경계에서만 불린다. `next`는 뒤 칸의 후보, `strokes`는 그 칸의 키 입력이고, 그 칸을 다시 매기는 경우 음수일 수 있다), `kanji::Dictionary`·`kanji::Connection`(아래 "일본어 변환"). `Engine::with_parts(interpreters, scorer)`로 갈아 끼운다.

### 점수

- 한국어와 영어 후보는 같은 눈금을 쓴다. 언어 모델의 자연로그 확률에 영문자 키마다 ln 26을 더한 값, 곧 "아무 키나 친 것"보다 몇 배 그럴듯한지의 로그다. 그래서 같은 키의 한국어 읽기와 영어 읽기를 바로 비교한다.
- 한국어: 음절마다 ln P(음절)의 합 + 키 수 × ln 26. P는 KS X 1001 완성형 2350자에 대한 음절 unigram이다 (`lm::ko_syllable`). 2350자 밖의 음절은 -15, 음절이 되지 못한 낱자모는 -12다. 한국어 단어 사전은 없다.
- 영어 뒤 한국어 조사: P_조사는 조사 문자열 전체의 unigram이다 (`lm::ko_particle`). 조사 둘이 이어진 것(`에서는` = `에서` + `는`)은 두 확률의 곱이다. 칸 끝의 숫자·구두점(ASCII)은 떼고 찾는다. 낱자모는 떼지 않으므로 `도ㅑㅜㅇ`은 조사가 아니다.
- 영어: 영문자 덩어리마다 ln P(단어) + 글자 수 × ln 26. P는 a-z와 단어 경계 27기호의 글자 trigram이다 (`lm::en_word`, 앞에 경계 둘, 뒤에 경계 하나). 대소문자는 무시한다. 숫자·구두점은 0점이다. 단어 목록 `english::SAMPLE_WORDS`에 든 단어는 3을 더 받는다.
- 일본어: 가장 그럴듯한 히라가나 읽기의 ln P + 키 수 × ln 26 + 사전 증거 - 6. P는 ぁ..ん 83자와 경계의 가나 bigram이다 (`lm::ja_reading`). 사전 증거는 변환 경로 비용이 키당 1800보다 싼 만큼을 500으로 나눈 값이다. 그보다 비싼 경로는 0이다. 미등록 노드가 섞인 경로가 그렇고, 표본 사전의 짧은 단어도 그렇다(`の`는 두 키에 4300). -6은 일본어 칸이 있으려면 치르는 값이다.
- 가나 bigram만으로는 언어를 가르지 못한다. ln P_가나 - ln P_영어는 `kitte`가 0.3이고 `tokyo`(ときょ)가 5.9다. 그래서 사전 단어인 `kitte`(경로 비용: 표본 사전 3700, Mozc 5728)는 사전 증거로 이기고, 사전 단어가 아닌 `tokyo`는 -6에 져서 영어로 남는다. 상수는 194개 표본 단어를 두 사전에서 돌려 맞췄다.
- 이전 공식 `글자 수 × 1.0 - cost / 2500`에서는 미등록 노드뿐인 읽기도 3~4글자면 영어 trigram보다 점수가 높았다. 그래서 `api` → `あぴ`, `node` → `ので`, Mozc에서 `engine` → `演技値`가 됐다.
- 남은 문제: Mozc에서 `node`는 여전히 `ので`다. `ので`는 비용 0인 사전 단어이고 경로 비용 3109가 `kitte`(5728)·`sushi`(5586)보다 싸다. 그래서 일본어 점수만으로는 `kitte`를 지키면서 `node`를 영어로 둘 수 없다. `sudo`는 `녀애`(한국어 -0.97)가 영어(-1.47)를 이긴다. 한국어와 영어 사이의 문제다.
- 짧은 단어는 경계 확률을 치르므로 한 음절 한국어와 가깝다. `git`은 영어(-2.0)가 `햣`(2350자 밖, -5.2)을 이긴다. `cm`(단어 목록 덕에 -2.2)과 `츠`(-2.5)는 거의 같아서, `10cmwjdeh`는 칸 하나인 `10츠정도`가 `10cm|정도`를 이긴다. `정도`는 조사가 아니라 명사이므로 위 조사 모델로는 고칠 수 없다.

### 언어 모델

- 표 네 개를 엔진에 넣는다. `crates/nullime-engine/data/ko_syllable.bin`(2350 바이트, KS X 1001 코드 순서), `ko_particle.txt`(1551 바이트, 조사 155개, 한 줄에 `조사\t비용`), `en_trigram.bin`(19683 바이트, `[(a×27+b)×27+c]`), `ja_bigram.bin`(7056 바이트, ぁ..ん 83자와 경계의 84기호, `[a×84+b]`). 비용은 `round(-ln p × 8)`이다.
- 조사 표는 빈도 조사의 조사 목록에서 만든다. 품사가 다른 동형어(주격 `이`, 보격 `이`)는 합치고, 낱자모 조사(`ㄴ`, `ㄹ`)는 뺀다.
- `tools/nullime-dict/gen_lm.py`가 만든다: `uv run python tools/nullime-dict/gen_lm.py --cache <dir>`. 원 자료는 `<dir>`에 내려받고 저장소에는 넣지 않는다. 표준 라이브러리만 쓴다. 내려받은 파일의 SHA-256이 스크립트의 `EXPECTED`와 다르면 멈추고, `--update`를 주면 그 파일로 만든다. Tatoeba는 매주 다시 내보내므로 새로 받으면 값이 달라진다. 지금 표는 2026-11-16(KST)에 내려받은 Tatoeba 내보내기로 만들었다. 파일 안에 내보낸 날짜가 없어서 받은 날짜와 SHA-256으로 판을 구분한다.
- 한국어 출처: Tatoeba 한국어 문장(CC BY 2.0 FR)과 국립국어원 「현대 국어 사용 빈도 조사」(2002) 결과 파일(공공누리 제1유형, 출처표시). 둘의 음절 확률을 반씩 섞고, 확률 질량 0.1%를 2350자에 고르게 나눠 두 자료에 없던 음절도 2350자 밖보다 높게 둔다. 빈도 조사는 표제어 단위라(`하다` → `하`), 동사·형용사의 `다`를 떼고 센다.
- 영어 출처: Tatoeba 영어 문장. 서로 다른 단어마다 `1 + ln(빈도)`의 무게로 trigram을 세고 interpolated Witten-Bell로 평활한다. 빈도를 그대로 쓰면 `the`의 모델이 되고, 단어마다 1로 세면 `no`가 오타만큼 드물어진다.
- 일본어 출처: Mozc OSS 사전의 읽기(`src/data/dictionary_oss/dictionary00.txt`~`dictionary09.txt`, 커밋 `a4541df29db443b49f81cf54665873b68fde9efa`, BSD-3 + IPAdic + 오키나와 사전 Public Domain). 히라가나로만 된 서로 다른 읽기(618072개)마다 한 번씩 bigram을 센다. 덧셈 평활한 unigram 위에 Witten-Bell로 평활한다. Tatoeba 일본어 문장은 가나 읽기가 없어서 쓰지 않았다. trigram은 `kitte`와 `tokyo`를 가르는 힘이 bigram과 비슷한데 표가 531 KB라 쓰지 않았다.
- 라이선스 조건: GPL·CC BY-SA 파생 자료는 쓰지 않는다. 출처와 표시 문구는 `NOTICE`에 있다. 국립국어원 「모두의 말뭉치」는 승인과 약정서가 있어야 내려받을 수 있고 결과물 공개에 사전 승인이 필요해 쓰지 않았다.

### 일본어 변환

Romaji 구간은 먼저 히라가나 읽기(여럿일 수 있음)가 되고, 읽기마다 형태소 lattice로 변환한다 (`ja_convert::convert`).

- `kanji::Dictionary::common_prefix(text, f)`는 `text`의 앞부분과 같은 읽기를 가진 모든 항목을 `(읽기 바이트 길이, Entry { surface, lid, rid, cost })`로 넘긴다. `surface`는 사전에서 빌린 문자열이거나 "읽기 그대로"·"읽기의 가타카나" 표시다. `unknown_id()`는 미등록 구간에 붙일 품사 id다.
- `kanji::Connection::cost(rid, lid)`는 앞 단어의 오른쪽 품사와 뒤 단어의 왼쪽 품사 사이 연접 비용이다. 문장 시작과 끝은 품사 id `bos_eos()`(기본 0)다.
- 구현은 둘이다. `SampleDictionary`는 엔진에 든 소량 사전과 5×5 연접표로, 테스트와 사전 없는 실행에 쓴다. `JaAsset`은 아래 "일본어 사전 애셋" 파일을 읽는다.
- lattice의 노드는 사전 항목과 미등록 노드다. 미등록 노드는 읽기 그대로(히라가나)와 가타카나 두 가지이고, 비용은 9000 + 글자당 400(가타카나는 600 더)이다. 1글자 노드는 모든 위치에 둔다. 2~8글자 노드는 그 위치에서 시작하는 사전 단어가 없을 때만 둔다. 그래서 어떤 읽기든 경로가 있다.
- Viterbi로 위치마다 최소 비용을 구하고, 뒤에서부터 A* 탐색으로 표기가 서로 다른 N-best를 비용순으로 낸다(탐색 큐에서 최대 4096번 꺼낸다). 노드 비용은 단어 비용 + 연접 비용의 합이다.
- 첫 읽기는 8개, 나머지 읽기는 2개까지 변환 후보를 낸다. 그 뒤에 히라가나 읽기와 가타카나를 가장 나쁜 변환 비용보다 1씩 큰 비용으로 붙이고, 같은 표기는 하나만 남긴다.
- `BaselineScorer`는 비용이 있는 일본어 후보를 위 "점수"의 일본어 공식(가나 bigram + 사전 증거)으로 점수 매긴다. 비용이 없는 후보는 이전 순위 공식을 쓴다.
- 문맥(앞서 확정한 텍스트)은 아직 쓰지 않는다.

## 일본어 사전 애셋

D15대로 사전은 엔진 밖의 파일 `nullime-ja.dict`다. 저장소에 넣지 않고(`.gitignore`) `tools/nullime-dict`로 Mozc OSS 사전(커밋 `a4541df29db443b49f81cf54665873b68fde9efa`의 `src/data/dictionary_oss`)에서 빌드한다. 라이선스 고지는 저장소 루트 `NOTICE`에 있고, 애셋 자체도 `license` 구역에 Mozc 사전 `README.txt`의 고지를 그대로 싣는다.

```sh
cargo run --release -p nullime-dict -- fetch --out <dir>
```

```sh
cargo run --release -p nullime-dict -- build --mozc <dir> --out nullime-ja.dict [--max-cost <n>]
```

```sh
cargo run --release -p nullime-dict -- eval --asset nullime-ja.dict --tsv <dir>/evaluation.tsv --ajimee <evaluation_items.json>
```

```sh
cargo run --release -p nullime-dict -- eval --asset nullime-ja.dict --mixed tools/nullime-dict/mixed.tsv
```

```sh
cargo run --release -p nullime-dict -- bench --asset nullime-ja.dict
```

- `build`는 쓴 파일을 다시 `JaAsset`으로 읽어 모든 연접 비용과 모든 항목을 확인한다. `--max-cost n`은 Mozc 비용이 n보다 큰(드문) 단어를 뺀다. 웹 데모의 작은 사전(`nullime-ja.small.dict`)은 `--max-cost 6000`이다.
- `eval`은 엔진의 `convert`를 직접 불러 1순위가 정답인지 센다 (D19). AJIMEE-Bench 파일은 저장소에 없고 경로로 받는다. AJIMEE의 `context_text`는 쓰지 않는다.
- `--mixed`는 언어 판정을 잰다. `tools/nullime-dict/mixed.tsv`의 줄마다 새 `Engine`에 키를 치고(공백은 스페이스 키라 확정한다), 앱에 남는 텍스트가 허용 답 중 하나인지 센다. 열은 `키 입력열\t답[\t다른 답…]`이고, `## 이름` 줄이 묶음을 시작한다. 묶음별·전체 정확도와 틀린 줄의 칸(`[text Lang]`)을 낸다. 위 두 평가셋과 달리 저장소에 있다.
- 사용자가 직접 빌드한 같은 형식의 파일도 그대로 꽂힌다 (D17). 로더는 Mozc 전용 가정을 하지 않는다.

형식 (버전 1, 모든 정수는 little-endian):

- 헤더 80바이트: 매직 `NULLIMEJ`, 버전 u32, 미등록 품사 id u16, 예약 u16, 그 뒤 8개 구역의 (오프셋 u32, 길이 u32). 구역은 8바이트 정렬이다.
- `license`: UTF-8 고지 텍스트.
- `charmap`: 자주 쓰는 읽기 글자 최대 254개의 코드 포인트(u32, 정렬). 트라이 키에서 이 글자는 1바이트(1부터 시작하는 순번)이고, 나머지 글자는 `0xFF` + UTF-8 바이트다.
- `trie`: yada 더블 배열. 키는 인코딩한 읽기, 값은 그룹 번호다.
- `groups`: 그룹 수 + 1개의 u32. 그룹 g의 항목은 `entries[groups[g]..groups[g+1]]`이고 비용순이다.
- `entries`: 항목당 8바이트 (surface_ref u32, 품사 u16, 비용 i16). surface_ref의 위 2비트가 0이면 `surfaces`의 오프셋(아래 30비트), 1이면 읽기 그대로, 2이면 읽기의 가타카나다.
- `pos`: (lid u16, rid u16) 쌍의 표. 항목의 품사 u16은 이 표의 순번이다.
- `surfaces`: (길이 u8, UTF-8 바이트)의 연속. 같은 표기는 한 번만 싣는다.
- `connection`: n u32, 행 기본값 n×i16, 행별 예외 시작 n×u32, 비트맵 n×⌈n/64⌉×u64, 블록마다 행 안에서 그 블록 앞의 예외 수 n×⌈n/64⌉×u16, 예외 비용 i16. 행(rid)마다 가장 흔한 값을 기본값으로 두고 다른 칸만 예외로 싣는다.

소유와 수명:

- `JaAsset`은 바이트 소유자를 `Arc`로 들고, 트라이와 표는 그 안을 가리킨다. 그래서 `Vec<u8>`이든 메모리 맵이든 복사 없이 쓰고, `JaAsset`을 복제해도 바이트는 공유한다.
- `JaAsset::new`는 `unsafe trait StableBytes`를 구현한 소유자만 받는다. 수명 내내 같은 바이트를 돌려준다는 약속이다. yada 트라이는 불러올 때 한 번 검사한 뒤 경계 검사 없이 읽기 때문이다. `Vec<u8>`, `Box<[u8]>`, `Arc<[u8]>`, `&'static [u8]`는 엔진이 구현해 두었다. 메모리 맵은 감싸는 쪽(Windows `MappedFile`)이 구현하고, 바이트가 바뀌지 않게 하는 책임도 그쪽이 진다.
- `JaAsset::new`는 헤더, 구역 경계, 표 크기, 트라이(yada 검사와 루트가 잎 노드가 아닌지)를 검사하고, 실패하면 `AssetError`를 돌려준다. 그 뒤 다른 표는 모두 경계 검사와 넘침 검사를 한 산술로 읽으므로, 검사를 통과한 손상 파일도 패닉하거나 범위 밖을 읽지 않는다. 잘못된 값은 항목을 건너뛰거나 연접 비용을 최대로 둔다. 읽기 하나가 내는 항목은 최대 512개다. 그룹이 비용순이라 잘라도 좋은 항목이 남는다. 손상 파일이 키 하나에 드는 일을 이렇게 묶는다. `JaAsset::from_bytes(&[u8])`는 복사해서 만든다.
- 웹은 `fetch`로 받은 바이트를 wasm 메모리로 옮겨 `Engine.withDictionary(bytes)`에 넘긴다. 엔진을 바꾸면 이전 엔진은 `free()`로 해제한다.
- Windows는 DLL과 같은 폴더의 `nullime-ja.dict`를 프로세스당 한 번 읽기 전용으로 메모리 맵하고(`memmap2`), 모든 텍스트 서비스가 그 `JaAsset`을 공유한다. 없거나 잘못된 파일이면 `OutputDebugString`에 이유를 남기고 샘플 사전으로 동작한다. 파일은 읽기 공유만 허용하는 공유 모드(`FILE_SHARE_READ`)로 열고, memmap2가 그 핸들을 맵 수명 동안 복제해 쥔다. 그래서 맵이 살아 있는 동안 Windows가 다른 프로세스의 쓰기·삭제·이름 바꾸기용 열기를 거부하고, `StableBytes` 약속을 운영체제가 지킨다. 다른 프로세스가 이미 쓰기용으로 연 파일이면 열기가 실패해 샘플 사전으로 동작한다. 사전을 바꾸려면 입력기를 쓰는 앱을 모두 닫은 뒤 바꿔 넣는다.

## 엔진 API

```rust
impl Engine {
    pub fn new() -> Engine; // 일본어는 내장 SampleDictionary
    pub fn with_ja_dictionary(asset: JaAsset) -> Engine;
    pub fn key(&mut self, ev: KeyEvent) -> Response;
    pub fn reset(&mut self) -> Response; // 포커스 이동 등에서 조합을 확정하고 닫는다
}
```

## 바인딩

플랫폼마다 표준 바인딩 도구를 쓴다. C ABI를 손으로 짜지 않는다.

- 웹: `crates/nullime-wasm`이 wasm-bindgen으로 `Engine`을 JS 클래스로 내보낸다. `Response`는 serde로 JS 객체가 된다. 필드 이름은 Rust 그대로(snake_case)이고, `lang`은 문자열 `"En"`/`"Ko"`/`"Ja"`, `preedit`가 없으면 `null`이다 (`serde_wasm_bindgen::Serializer::json_compatible`). JS 쪽 API는 `new Engine()`, `Engine.withDictionary(bytes)`(`Uint8Array`, 잘못된 파일이면 예외), `engine.dictionaryEntries`(사전 항목 수, 샘플 사전이면 `undefined`), `engine.key(key, ch, shift, modifier)`(key: 0=Other 1=Char 2=Space 3=Enter 4=Backspace 5=Left 6=Right 7=Up 8=Down, ch: key가 1일 때 ASCII 코드), `engine.reset()`이다. 빌드는 `cargo build --target wasm32-unknown-unknown` 후 `wasm-bindgen --target web`이다.
- Windows: `platform/windows`가 `nullime-engine`을 Rust 의존성으로 직접 쓴다.
- macOS(나중, D14): UniFFI로 Swift 바인딩을 만든다.

## Windows

`platform/windows`(패키지 `nullime-windows`)는 TSF 텍스트 입력 처리기(TIP) DLL이다. 엔진 크레이트를 직접 쓰고, ko-KR(LANGID 0x0412) 입력 프로필 하나로 등록된다.

- 모듈은 도메인별로 나눈다: `dictionary`(DLL 옆 `nullime-ja.dict` 메모리 맵, 위 "일본어 사전 애셋"), `server`(클래스 팩토리, `DllGetClassObject`/`DllCanUnloadNow`), `registration`(`DllRegisterServer`/`DllUnregisterServer`, CLSID·프로필 GUID), `keymap`(가상 키 → 엔진 키, 순수 함수), `service`(TSF 연결: 활성화, 키 이벤트, 포커스 변경), `composition`(편집 세션에서 `Response`를 문서에 반영), `display_attribute`(입력 밑줄, 포커스된 칸 굵은 밑줄), `candidate_window`(후보 창).
- `OnTestKeyDown`은 엔진을 건드리지 않고 먹을지 예측한다. 조합 중이면 모든 키, 아니면 수식키 없는 `Char`만 먹겠다고 답한다. `OnKeyDown`이 엔진을 부르고, 동기 편집 세션(`TF_ES_SYNC`)으로 확정·조합을 반영한 뒤 `Response.handled`를 그대로 돌려준다. 그래서 확정 뒤의 스페이스·엔터는 앱이 받는다 (D5). 동기 세션이 거절되면 비동기로 넘긴다.
- Shift, Ctrl, Alt, Win, Caps Lock 키 자체를 누른 것은 엔진에 넘기지 않는다. 넘기면 `Other`가 되어 Shift+Q를 치기 전에 조합이 확정된다.
- 문서 포커스가 바뀌면 `engine.reset()`의 확정 텍스트로 조합을 끝낸다. 앱이 조합을 먼저 끝내면(`OnCompositionTerminated`) 조합 창 텍스트가 곧 확정 텍스트이므로 그대로 두고 엔진만 비운다.
- 후보 창은 포커스를 가져가지 않는 자체 Win32 팝업(`WS_POPUP`, `WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE`)이다. `ITfContextView::GetTextExt`로 얻은 포커스된 칸 아래에 뜨고, 그 칸의 후보와 언어 태그를 보여주며 선택 후보를 강조한다.

### 설치

빌드는 `cargo build -p nullime-windows --release`이고 산출물은 `target\release\nullime_windows.dll`(64비트)이다. 등록은 레지스트리를 쓰므로 사용자가 관리자 권한 명령 프롬프트에서 저장소 루트로 이동해 직접 한다.

```
regsvr32 "%CD%\target\release\nullime_windows.dll"
```

해제는 다음과 같다. DLL을 다시 빌드하기 전에도 해제하거나, 입력기를 쓰는 앱을 모두 닫는다 (로드된 DLL은 덮어쓸 수 없다).

```
regsvr32 /u "%CD%\target\release\nullime_windows.dll"
```

등록 뒤 설정 > 시간 및 언어 > 언어 및 지역에서 한국어의 언어 옵션을 열고 키보드 추가에서 NullIME을 고른다. 한국어가 목록에 없으면 먼저 언어 추가로 한국어를 넣는다. 입력할 때는 Win+Space로 NullIME을 고른다.

## 웹 데모

`web/`에 정적 데모 페이지(`index.html`, `main.js`, `style.css`)가 있다. 빌드와 실행은 `web/README.md`를 따른다. 페이지는 `event.code`로 물리 키를 US QWERTY 문자로 바꿔 `engine.key()`에 넘기고, `Response`의 `commit`을 자체 편집기에 넣은 뒤 `handled = false`면 기본 동작(공백, 줄바꿈, 글자 삭제, 커서 이동)을 직접 적용한다. 조합 창은 칸마다 언어별 색을 입히고, 포커스된 칸 아래에 후보 목록을 띄운다. 마지막 `Response` JSON은 디버그 패널에 보인다.

시작할 때 `assets/nullime-ja.small.dict`를 받아 엔진을 바꾸고, 실패하면 샘플 사전으로 남는다. 상태 줄은 "사전 로드됨: …" 또는 "사전 없음: 샘플 사전 사용"이다. "전체 사전 받기" 버튼은 `assets/nullime-ja.dict`를 받아 바꾸고, 파일 선택 칸은 사용자의 `.dict` 파일을 읽어 바꾼다 (D17, D18). 바꾸기 전에 조합 중인 텍스트를 확정한다.
