# NullIME 구조

스펙은 `docs/spec.md`에 있다. 이 문서는 엔진과 플랫폼 구현 사이의 경계를 정한다.

## 디렉터리

```
Cargo.toml                 workspace
crates/nullime-engine      순수 Rust 엔진. 플랫폼 코드 없음.
crates/nullime-ffi         C ABI. wasm32(웹)과 macOS가 쓴다. 결과는 JSON 문자열.
web/                       웹 데모. 정적 HTML + JS 글루 + nullime_ffi.wasm
platform/windows           TSF 텍스트 서비스 DLL (Rust, `windows` 크레이트). 엔진 크레이트를 직접 쓴다.
platform/macos             InputMethodKit 입력기 (Swift). libnullime_ffi.a를 링크한다.
```

## 키 모델

플랫폼은 물리 키를 US QWERTY 기준 문자로 바꿔서 엔진에 넘긴다. OS 자판 배열과 무관하게 같은 키는 같은 값이 된다.

```rust
pub enum Key { Letter(u8 /* b'a'..=b'z' */), Space, Enter, Backspace, Left, Right, Up, Down, Other }
pub struct KeyEvent { pub key: Key, pub shift: bool, pub modifier: bool /* Ctrl/Alt/Cmd */ }
```

인쇄 가능한 비영문 문자와 모든 기타 키는 `Key::Other`로 넘긴다.

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
pub struct Candidate { pub text: String, pub lang: Lang }
```

키별 동작:

- `Letter`: 키 입력열에 추가하고 다시 해석한다 (D4). `handled = true`.
- `Space`, `Enter`: 조합 중이면 각 칸의 선택 후보를 이어 붙여 `commit`으로 내고 `handled = false` (D5: 키 자체는 앱이 받는다). 조합 중이 아니면 `handled = false`.
- `Backspace`: 조합 중이면 마지막 키 입력을 취소한다 (D9). 비면 조합을 끝낸다. 조합 중이 아니면 `handled = false` (D10).
- `Left`/`Right`: 조합 중이면 `focused` 이동 (D8). 아니면 `handled = false`.
- `Up`/`Down`: 조합 중이면 포커스된 칸의 `selected` 이동 (D8). 아니면 `handled = false`.
- `Other` 또는 `modifier = true`: 조합 중이면 확정 텍스트를 내고 `handled = false`.

## 해석 (lattice)

- 키 입력열 `keys[0..n]`의 모든 구간 `keys[i..j]`에 대해 각 `Interpreter`(영어, 두벌식, Romaji+한자)가 후보를 낸다.
- `Scorer` 트레이트가 후보 점수를 매긴다 (D7: 교체 가능). MVP scorer는 임시 휴리스틱과 소량 임베드 단어 목록이다.
- 동적 계획법으로 최고 점수 경로를 고른다. 그 경로의 구간들이 칸이 된다. 칸이 늘수록 벌점을 준다.
- 각 칸의 후보 목록은 같은 구간에 대한 모든 언어의 후보를 점수순으로 담는다.
- 사용자가 상하 화살표로 고른 칸은 고정된다. 이후 키 입력은 마지막 고정 칸 뒤 구간만 다시 해석한다. 백스페이스가 고정 칸 안으로 들어가면 그 칸의 고정을 푼다.

## FFI (C ABI)

```c
NullimeEngine* nullime_engine_new(void);
void           nullime_engine_free(NullimeEngine*);
// key: 0=Other 1=Letter 2=Space 3=Enter 4=Backspace 5=Left 6=Right 7=Up 8=Down
// letter: b'a'..=b'z' (key==1일 때만)
// 반환: Response JSON (UTF-8). nullime_string_free로 해제한다.
NullimeString  nullime_engine_key(NullimeEngine*, uint32_t key, uint32_t letter, bool shift, bool modifier);
NullimeString  nullime_engine_reset(NullimeEngine*); // 포커스 이동 등에서 조합을 확정하고 Response JSON 반환
void           nullime_string_free(NullimeString);
typedef struct { const uint8_t* ptr; size_t len; } NullimeString;
```

wasm32에서는 같은 함수에 `nullime_alloc`/`nullime_dealloc`을 더해 JS가 선형 메모리에서 JSON을 읽는다. wasm-bindgen은 쓰지 않는다.
