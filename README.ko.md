*[English version](README.md)*

# Staveloom

**Staveloom**은 Rust로 작성된 MusicXML 파싱 및 SVG 렌더링 엔진입니다. CLI 도구와 브라우저 기반 대화형 웹 플레이어를 함께 제공하며, 출판 수준의 고품질 악보 생성을 목표로 합니다.

> **웹 플레이어는 완전히 클라이언트 사이드로 동작합니다.** 렌더링(WASM), MIDI 합성(SpessaSynth AudioWorklet), 오디오 재생 모두 서버 없이 브라우저 안에서 완결됩니다.

---

## 주요 기능

### 1. 정교한 파싱 및 유연한 데이터 모델

- MusicXML 3.1/4.0 표준 광범위 지원, 압축 `.mxl` 아카이브 직접 파싱.
- **비표준 구조 대응**: 다양한 악보 소프트웨어에서 내보낸 특이한 XML 트리 구조를 유연하게 파싱.
- **선택적 파트 렌더링**: `--parts P1,P2` 플래그로 특정 파트만 추출하여 SVG, MIDI, Audio 동시 적용.

### 2. 전문적인 레이아웃 엔진

- **컴팩트 레이아웃 (기본값)**: 각 음표의 실제 렌더링 너비에 딱 맞게 간격을 할당하여 가로 밀도 극대화.
- **탄력적 레이아웃 (`--elastic`)**: 음값(duration)에 따른 시각적 균형을 위해 $W \propto duration^{0.6}$ 비선형 시간-공간 매핑.
- **모바일 레이아웃 (`--mobile`)**: 좁은 화면 전용 프리셋 — 여백/간격을 추가로 압축하고, 두 번째 시스템부터는 파트 이름을 생략해 컨텐츠 영역을 최대화. 뷰포트 폭 기반 자동 감지 또는 수동 토글 지원(웹 플레이어).
- **우선순위 기반 수직 스태킹 (11-pass)**: Dynamics → Octave Shift → Dashes/Bracket → Wedge → Pedal → Segno/Coda → Words → Harmony → Lyrics → Metronome → Rehearsal 순으로 `OccupancyMap`을 통해 충돌 없이 배치.
- **정확한 빔/슬러 렌더링**: 빔 그룹 내 줄기 방향 통일(다성부 충돌 방지 포함), 슬러 끝점을 올바른 음표머리/스템 팁에 앵커링, 그랜드 스태프 크로스 스태프 빔의 스템 방향 자동 처리.
- **고도화된 레이아웃**: Nested Tuplets (Staggered brackets), Dynamic Wavy Line (Trill 연동), Rhythmic Slash Notation, Optimized Multi-measure Rests, Visual Octave Shift (8va/8vb).

### 3. 오디오 및 MIDI 렌더링 (CLI)

- **고품질 MP3 합성**: `rustysynth` (SF2) + LAME 인코딩.
- **표준 MIDI 내보내기**: `midly` 기반 멀티트랙 MIDI(SMF Format 1). `Trill`, `Mordent`, `Arpeggio`, `Tremolo` 등을 실제 음표 시퀀스로 변환.
- **아티큘레이션**: Staccato (길이 50%), Tenuto (100%), Accent (Velocity +30%), Fermata 연장.

### 4. 플레이 메타데이터 및 오디오 동기화

- **Timeline Solver**: `<repeat>`, Volta Ending, D.S., D.C., Segno, Coda 등 복잡한 연주 순서를 완벽하게 해석.
- **Beat Sync**: 모든 정규 박자 및 음표 Onset의 SVG 좌표(x, y_start, y_end) + 절대 시간(seconds) 추출.
- **Dynamic Tempo Tracking**: `<sound tempo>` 및 메트로놈 기호를 분석하여 누적 시간 계산.
- **Anacrusis Support**: 못갖춘마디(`implicit`) 감지 및 시작 오프셋 자동 조정.

### 5. 대화형 웹 플레이어

완전한 클라이언트 사이드 아키텍처로, 4개 Phase에 걸쳐 구현되었습니다.

#### 렌더링 (Phase 1)

- Rust 렌더러를 WASM으로 빌드(`wasm-pack`). `staveloom_wasm_bg.wasm` (1.1 MB) 브라우저 내 실행.
- MusicXML 드래그앤드롭 → WASM 파싱/렌더링 → SVG 즉시 표시.
- Elastic/Compact/Mobile 레이아웃 토글(뷰포트 폭 기반 모바일 자동 감지 포함), 페이지 너비 슬라이더, 가로 한 줄 스크롤 모드.
- 파트 필터링 체크박스, SVG·MIDI 다운로드.

#### 클라이언트 사이드 MIDI 합성 (Phase 2)

- **SpessaSynth** (AudioWorklet 기반) SF2 실시간 합성. 악기별 SF2 온디맨드 로딩.
- **SF2 병합 전략**: 여러 SF2를 `mergeSF2Buffers()`로 단일 SF2 병합 후 `addSoundBank()` 1회 호출 → 채널 리셋 방지.
- **실시간 커서 동기화**: `currentHighResolutionTime` + `audioLatency` 보정으로 sub-frame 정밀도. 재생 시작 직후 클록 재보정으로 초기 지연 최소화, 커서가 화면 밖으로 벗어나면 자동 스크롤(파트가 많아 한 화면에 안 들어오면 상단 정렬).
- **iOS 오디오 처리**: `unlockAudio()`를 사용자 제스처 동기 콜스택에서 호출하여 AudioContext 생성. `play()` async화로 `await resume()` 후 재생. `touchstart` 리스너로 백그라운드 후 재개 처리.

#### 가상 SVG 렌더링 (Phase 3)

대용량 악보의 메모리·DOM 부하를 최소화하는 3-tier 전략:

| 시스템 수                    | 렌더링 방식                                                 |
| ---------------------------- | ----------------------------------------------------------- |
| 1개                          | 단일 SVG 직접 마운트                                        |
| 2~10개 (`COMBINE_THRESHOLD`) | `_combineSystems()`로 단일 SVG 결합                         |
| 11개 이상                    | IntersectionObserver 가상화 (뷰포트 내 시스템만 DOM 마운트) |

- `rootMargin` ≈ 2 시스템 높이로 동적 계산하여 선행 렌더링.
- `fullReset` 플래그로 파일 교체 시에만 스크롤·MIDI 리셋 (레이아웃 토글 시 위치 보존).

#### PWA 오프라인 지원 (Phase 4)

- **Service Worker** (`sw.js`): Cache-First + 동적 캐싱. 16개 정적 자산 사전 캐시. `ignoreSearch: true`로 `?v=N` 버전 쿼리 무시. SF2 파일은 첫 사용 시 동적 캐시.
- **PWA Manifest**: `display: standalone`, 192·512px 아이콘. 홈 화면 설치 지원.

#### Bravura 폰트 번들링

- 시스템에 Bravura 미설치 환경 대응: OTF(500.9 KB) → WOFF2 subset (28.3 KB, 212 glyph, 94.3% 절감).
- `@font-face { src: local('Bravura'), url('./fonts/Bravura-subset.woff2') }`: 로컬 우선, URL 폴백.
- `<link rel="preload">` + `font-display: block`으로 첫 렌더 전 폰트 확보, FOUT 방지.

#### 모바일 UX

- **사이드바 토글**: 모바일(≤768px) 기본 숨김. 좌상단 햄버거(☰) 버튼으로 오버레이 슬라이드 인.
- **2줄 툴바**: 1줄: 시간 + 재생. 2줄: 볼륨/줌/다운로드.
- **Safe Area**: `viewport-fit=cover` + `env(safe-area-inset-bottom)` + `100dvh`로 iOS Safari URL bar 및 홈 인디케이터 대응.

### 6. 검증 및 리포팅

- JSON/SVG/Timeline/Metadata 스냅샷 회귀 테스트 (100% 통과).
- W3C 표준 샘플 기반 시각적 검증 리포트 (`specification_report.html`).
- `preview.html` (전체 SVG 탐색), `preview_debug.html` (동기화 라인 시각화).

---

## 설치 및 사용법

### CLI 빌드

```bash
cargo build --release
```

### CLI 명령어 옵션

```bash
# 기본 렌더링 (SVG 생성)
cargo run -- <파일.musicxml> --output out.svg

# 특정 파트만 선택
cargo run -- <파일.musicxml> --parts P1,P2 --output filtered.svg

# MIDI 및 MP3 오디오 생성 (CLI 전용)
cargo run -- <파일.musicxml> --midi out.mid --sf2 path/to/font.sf2 --audio out.mp3

# 동기화 메타데이터 추출 (JSON)
cargo run -- <파일.musicxml> --metadata sync_data.json

# 파트 목록 확인
cargo run -- <파일.musicxml> --list-parts

# 탄력적 레이아웃 사용
cargo run -- <파일.musicxml> --elastic

# 모바일 레이아웃 사용 (좁은 화면용 조밀 배치)
cargo run -- <파일.musicxml> --mobile

# 가로 한 줄 모드
cargo run -- <파일.musicxml> --horizontal

# 페이지 너비 지정 (기본값: 1200)
cargo run -- <파일.musicxml> --width 800

# 라이선스 및 서드파티 고지 확인
cargo run -- --license
```

### 웹 플레이어 실행

```bash
# WASM 빌드 (최초 1회 또는 Rust 코드 변경 시)
bash wasm-build.sh

# 개발 서버 시작
python3 server.py
```

브라우저에서 `http://localhost:8000` 접속 후 `.xml`, `.musicxml`, `.mxl` 파일을 드래그앤드롭하면 즉시 사용 가능합니다.

### Bravura 폰트 Subset 재생성

```bash
# fonttools 필요: pip install fonttools brotli
bash scripts/generate_bravura_subset.sh
# 또는 커스텀 경로 지정:
bash scripts/generate_bravura_subset.sh /path/to/Bravura.otf
```

---

## 테스트 및 검증

### 전체 테스트 실행

```bash
cargo test
```

### 특정 샘플 테스트

```bash
TEST_FILTER=bend-element cargo test
```

### 시각적 검증 리포트 생성

```bash
python3 scripts/generate_report.py
# → specification_report.html 생성
```

### SVG 미리보기

```bash
python3 scripts/generate_preview.py
# → preview.html 생성
```

### 메타데이터 동기화 디버깅

```bash
cargo test test_all_samples_debug_svg_snapshot
python3 scripts/generate_debug_preview.py
# → preview_debug.html 생성
```

---

## 기술 스택

- **Core**: Rust (Edition 2024) — roxmltree, serde, svg, midly, rustysynth, lame
- **WASM**: wasm-pack, wasm-bindgen (`wasm32-unknown-unknown`)
- **Web Player**: Vanilla JS (ES2022), SpessaSynth (AudioWorklet), Service Worker
- **Font**: SMuFL / Bravura (WOFF2 subset, fonttools pyftsubset)
- **Standard**: MusicXML 3.1/4.0
- **Dev Server**: Python 3 표준 라이브러리

---

## 라이선스

이 프로젝트는 **MIT OR Apache-2.0** 듀얼 라이선스로 배포됩니다 — 원하는
쪽을 선택해서 사용하시면 됩니다. 전문은 [`LICENSE-MIT`](LICENSE-MIT),
[`LICENSE-APACHE`](LICENSE-APACHE)를 참고하세요.

SpessaSynth, Bravura 폰트, FluidR3 GM 사운드폰트 등 번들된 서드파티
오픈소스에 대한 라이선스 검토는 [`docs/THIRDPARTY_LICENSE.md`](docs/THIRDPARTY_LICENSE.md)에
정리되어 있습니다.
