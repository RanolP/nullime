# NullIME 구조

스펙은 `docs/spec.md`에 있다. 이 문서는 엔진과 플랫폼 구현 사이의 경계를 정한다.

## 디렉터리

```
Cargo.toml                 workspace
crates/nullime-engine      순수 Rust 엔진. 플랫폼 코드 없음.
crates/nullime-wasm        wasm-bindgen 바인딩. 웹 데모가 쓴다.
web/                       웹 데모. 정적 HTML + wasm-bindgen 산출물
platform/windows           TSF 텍스트 서비스 DLL (Rust, `windows` 크레이트). 엔진 크레이트를 직접 쓴다.
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
pub struct Candidate { pub text: String, pub lang: Lang }
```

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
- `Scorer` 트레이트가 후보 점수를 매긴다 (D7: 교체 가능). MVP scorer는 임시 휴리스틱과 소량 임베드 단어 목록이다.
- 동적 계획법으로 최고 점수 경로를 고른다. 그 경로의 구간들이 칸이 된다. 칸이 늘수록 벌점을 준다.
- 각 칸의 후보 목록은 같은 구간에 대한 모든 언어의 후보를 점수순으로 담는다.
- 사용자가 상하 화살표로 고른 칸은 고정된다. 이후 키 입력은 마지막 고정 칸 뒤 구간만 다시 해석한다. 백스페이스가 고정 칸 안으로 들어가면 그 칸의 고정을 푼다.
- 마지막 고정 칸 앞에 있는 고정되지 않은 칸도 다시 해석하지 않는다. 마지막 고정 칸까지가 통째로 굳는다.
- 한 칸은 최대 32 키 입력이다 (`decode::MAX_SPAN`).
- 해석은 점진적이다 (`decode::Lattice`). 위치 j까지의 최고 경로는 `keys[..j]`에만 달려 있으므로, 키 하나를 더하면 그 키에서 끝나는 구간(최대 32개)만 계산하고 백스페이스는 마지막 노드를 버린다. 고정 칸이 바뀌어 다시 해석할 시작점이 달라지면 그 뒤를 새로 계산한다. 결과는 매번 처음부터 해석한 것과 같다.
- 어떤 해석기도 읽지 못한 구간은 키를 입력한 그대로(`En`) 낮은 점수(키당 -10)의 후보 하나로 둔다. 그래서 모든 칸은 후보가 하나 이상이다.
- `kanji::Dictionary::lookup(&self, reading) -> Vec<&str>`는 `self`에서 빌린 문자열을 돌려준다. 실행 중에 읽어 들인 사전도 구현할 수 있다.
- 교체 지점: `Interpreter`(구간 → 후보, 언어별 선호 순서), `Scorer`(`score(strokes, candidate, rank)`와 `segment_penalty()`), `kanji::Dictionary`(읽기 → 한자). `Engine::with_parts(interpreters, scorer)`로 갈아 끼운다.

## 엔진 API

```rust
impl Engine {
    pub fn new() -> Engine;
    pub fn key(&mut self, ev: KeyEvent) -> Response;
    pub fn reset(&mut self) -> Response; // 포커스 이동 등에서 조합을 확정하고 닫는다
}
```

## 바인딩

플랫폼마다 표준 바인딩 도구를 쓴다. C ABI를 손으로 짜지 않는다.

- 웹: `crates/nullime-wasm`이 wasm-bindgen으로 `Engine`을 JS 클래스로 내보낸다. `Response`는 serde로 JS 객체가 된다. 필드 이름은 Rust 그대로(snake_case)이고, `lang`은 문자열 `"En"`/`"Ko"`/`"Ja"`, `preedit`가 없으면 `null`이다 (`serde_wasm_bindgen::Serializer::json_compatible`). JS 쪽 API는 `new Engine()`, `engine.key(key, ch, shift, modifier)`(key: 0=Other 1=Char 2=Space 3=Enter 4=Backspace 5=Left 6=Right 7=Up 8=Down, ch: key가 1일 때 ASCII 코드), `engine.reset()`이다. 빌드는 `cargo build --target wasm32-unknown-unknown` 후 `wasm-bindgen --target web`이다.
- Windows: `platform/windows`가 `nullime-engine`을 Rust 의존성으로 직접 쓴다.
- macOS(나중, D14): UniFFI로 Swift 바인딩을 만든다.

## Windows

`platform/windows`(패키지 `nullime-windows`)는 TSF 텍스트 입력 처리기(TIP) DLL이다. 엔진 크레이트를 직접 쓰고, ko-KR(LANGID 0x0412) 입력 프로필 하나로 등록된다.

- 모듈은 도메인별로 나눈다: `server`(클래스 팩토리, `DllGetClassObject`/`DllCanUnloadNow`), `registration`(`DllRegisterServer`/`DllUnregisterServer`, CLSID·프로필 GUID), `keymap`(가상 키 → 엔진 키, 순수 함수), `service`(TSF 연결: 활성화, 키 이벤트, 포커스 변경), `composition`(편집 세션에서 `Response`를 문서에 반영), `display_attribute`(입력 밑줄, 포커스된 칸 굵은 밑줄), `candidate_window`(후보 창).
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
