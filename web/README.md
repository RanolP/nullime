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
