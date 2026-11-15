# NullIME 웹 데모

`crates/nullime-wasm`을 wasm-bindgen으로 빌드해 `web/pkg/`에 내보내고, 정적 서버로 `web/`을 연다. 번들러와 npm 의존성은 없다.

## 빌드

저장소 루트에서 실행한다.

```sh
cargo build --release --target wasm32-unknown-unknown -p nullime-wasm
```

```sh
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/nullime_wasm.wasm
```

### 일본어 사전

사전 애셋은 저장소에 없다(`.gitignore`). 없으면 데모는 내장 샘플 사전으로 동작하고 상태 줄에 "사전 없음: 샘플 사전 사용"을 띄운다. 만들려면 Mozc OSS 사전을 받아 두 애셋을 빌드한다. 받는 위치는 저장소 밖이어도 된다.

```sh
cargo run --release -p nullime-dict -- fetch --out ../mozc-dictionary-oss
```

```sh
cargo run --release -p nullime-dict -- build --mozc ../mozc-dictionary-oss --out web/assets/nullime-ja.dict
```

```sh
cargo run --release -p nullime-dict -- build --mozc ../mozc-dictionary-oss --out web/assets/nullime-ja.small.dict --max-cost 6000
```

페이지는 시작할 때 작은 사전(`nullime-ja.small.dict`, 약 8 MB)을 받는다. "전체 사전 받기"는 전체 사전(`nullime-ja.dict`, 약 34 MB)으로 바꾼다. 파일 선택 칸에는 같은 형식으로 직접 빌드한 사전을 넣을 수 있다.

## 실행

```sh
npx serve web
```

```sh
python -m http.server -d web 8000
```

브라우저에서 출력된 주소(`python`은 `http://localhost:8000`)를 연다. `file://`로는 ES 모듈과 wasm이 로드되지 않는다.

## 사용

- OS 입력 언어는 영어로 둔다. 페이지가 물리 키를 직접 읽고 기본 동작을 막는다.
- 입력 영역을 클릭한 뒤 타자를 친다. 좌우는 칸, 상하는 후보를 고른다.
- Space와 Enter는 조합을 확정한 뒤 공백과 줄바꿈을 넣는다. Backspace는 조합 중에는 마지막 키 입력을 취소하고, 아니면 글자를 지운다.
- 아래 디버그 패널에 마지막 `Response` JSON이 보인다.
