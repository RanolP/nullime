# NullIME 웹 데모

`crates/nullime-wasm`을 wasm-bindgen으로 빌드해 `web/pkg/`에 내보내고, 정적 서버로 `web/`을 연다. 번들러와 npm 의존성은 없다.

## 빌드

저장소 루트에서 실행한다. wasm 빌드, `wasm-bindgen`(`mise.toml`이 버전을 고정), Mozc 사전 내려받기, 두 사전 애셋 빌드를 한 번에 한다. GitHub Pages 워크플로(`.github/workflows/pages.yml`)도 같은 스크립트를 쓴다.

```sh
scripts/build-web.sh
```

Mozc 파일은 `target/mozc`에 받는다(인자나 `MOZC_DIR`로 바꿀 수 있다). 그 폴더의 스탬프가 고정된 커밋(`tools/nullime-dict/src/main.rs`의 `MOZC_COMMIT`)과 같으면 다시 받지 않는다.

### 일본어 사전

사전 애셋은 저장소에 없다(`.gitignore`). 없으면 데모는 내장 샘플 사전으로 동작하고 상태 줄에 "사전 없음: 샘플 사전 사용"을 띄운다. 위 스크립트가 `web/assets/`에 두 애셋을 만든다.

페이지는 시작할 때 작은 사전(`nullime-ja.small.dict`, 약 8 MB)을 받는다. "전체 사전 받기"는 전체 사전(`nullime-ja.dict`, 약 34 MB)으로 바꾼다. 파일 선택 칸에는 같은 형식으로 직접 빌드한 사전을 넣을 수 있다.

## 실행

```sh
npx serve web
```

```sh
python -m http.server -d web 8000
```

브라우저에서 출력된 주소(`python`은 `http://localhost:8000`)를 연다. `file://`로는 ES 모듈과 wasm이 로드되지 않는다.

## GitHub Pages

`main`에 푸시하면 `pages.yml`이 테스트(`cargo test --workspace --exclude nullime-windows`)와 `scripts/build-web.sh`를 돌리고 `web/`을 그대로 올린다. 저장소 Settings → Pages → Source를 "GitHub Actions"로 둔다. 페이지의 파일 URL은 모두 상대 경로라 `/<저장소>/` 하위 경로에서도 동작한다. Pages 한도는 파일 100 MB, 사이트 1 GB이고 전체 사전은 약 34 MB다.

## 사용

- OS 입력 언어는 영어로 둔다. 페이지가 물리 키를 직접 읽고 기본 동작을 막는다.
- 입력 영역을 클릭한 뒤 타자를 친다. 좌우는 칸, 상하는 후보를 고른다.
- Space와 Enter는 조합을 확정한 뒤 공백과 줄바꿈을 넣는다. Backspace는 조합 중에는 마지막 키 입력을 취소하고, 아니면 글자를 지운다.
- 아래 디버그 패널에 마지막 `Response` JSON이 보인다.
