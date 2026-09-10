# Staveloom — AI Agent Context

Staveloom은 Rust로 만들어진 MusicXML 파싱 및 SVG 렌더링 엔진으로, CLI 도구와 브라우저 기반 대화형 웹 플레이어를 함께 제공합니다.

---

## 현재 구현 상태

### 1. 파싱 및 데이터 모델링

- **모델 고도화**: `Note` 구조체에 `grace`, `is_cue`, `harmonies`, `unpitched` 등 전문 필드 지원.
- **Technical & Articulation**: `Slide`, `Smear`, `SnapPizzicato`, `SoftAccent`, `Spiccato`, `Staccatissimo`, `Stress`, `Unstress`, `Tap`, `Bend` (Pre-bend, Release 포함) 등 방대한 주법 모델링.
- **Figured Bass & Numeral**: `suffix`, `prefix`, `default-y`, `extend` 속성 지원으로 고전 음악 기보 강화.
- **Non-traditional Keys**: `key-step`, `key-alter`, `key-octave`를 통한 자유로운 조표 구성 지원.
- **유연한 파싱**: 비표준 MusicXML 구조(예: `<technical>`이 `<note>` 직계 자식인 경우 등)에 대한 재귀적 파싱 및 예외 처리.
- **오디오 모델 캡처**: `instrument-sound`, `midi-instrument`, `dynamics`, `ornaments` 등 연주에 필요한 핵심 메타데이터 파싱.
- **선택적 파트 렌더링**: `--parts P1,P2` CLI 플래그를 통해 특정 파트만 추출하여 SVG, MIDI, Audio로 렌더링.

### 2. 플레이 메타데이터 및 타임라인 분석 (오디오 동기화)

- **Timeline Solver**: `<repeat>` (forward/backward), `<ending>` (Volta), `D.S.`, `D.C.`, `Segno`, `Coda` 등을 해석하여 실제 연주되는 마디 인덱스 시퀀스 생성.
- **Precision Play Metadata**: 곡의 모든 정규 박자 및 개별 음표 시작 지점(Onset)에 대한 물리적 SVG 좌표(x, y_start, y_end) 추출.
- **Dynamic Tempo Tracking**: 마디별 템포 변화를 실시간으로 추적하여 곡의 시작부터 각 이벤트까지의 정확한 절대 시간(seconds) 계산.
- **Anacrusis Support**: 못갖춘마디(`implicit`)를 감지하여 타임라인 시작 오프셋 자동 조정.

### 3. 오디오 및 MIDI 렌더링 엔진

- **High-Fidelity MIDI 시퀀싱**: `midly` 기반의 멀티트랙 MIDI(SMF Format 1) 생성.
- **음악적 뉘앙스 구현**: `Trill`, `Mordent`, `Arpeggio`, `Tremolo` 등을 실제 MIDI 음표 시퀀스로 변환.
- **아티큘레이션 변조**: `Staccato` (길이 50%), `Tenuto` (100%), `Accent` (Velocity +30%) 등의 주법 자동 반영.
- **SoundFont 기반 MP3 합성 (CLI)**: `rustysynth` + LAME 인코딩으로 PCM → MP3 변환. `ppp` ~ `fff` 다이내믹스를 MIDI Velocity(16~126)로 정밀 매핑.

### 4. SVG 렌더링 엔진

- **Elastic Layout**: Physical Space Analysis → 마디별 Prefix/Body/Suffix 공간 사전 할당 → $W = C \cdot duration^{0.6}$ 비선형 간격 → Dynamic System Breaking.
- **Priority Vertical Stacking**: Dynamics → Octave Shift → Dashes/Bracket → Wedge → Pedal → Segno/Coda → Words → Harmony → Lyrics → Metronome → Rehearsal 순 11-pass 배치. `OccupancyMap`으로 충돌 방지.
- **Guitar/TAB Specialization**: Professional Bend Rendering (곡선/Pre-bend/Release), Intelligent Stem Suppression, Capo Support, Hammer-on/Pull-off/Tap 정밀 배치.
- **Advanced Layout**: Rhythmic Slash Notation, Optimized Multi-measure Rests, Visual Octave Shift (8va/8vb), Nested Tuplets (Staggered brackets), Dynamic Wavy Line (Trill 연동).

### 5. 웹 플레이어 (브라우저 클라이언트 사이드)

전체 렌더링과 합성이 브라우저 안에서 완결됩니다. 서버는 정적 파일 제공 전용입니다.

#### Phase 1 — WASM 렌더링 기반 (완료)

- Rust 렌더러를 `wasm-pack`으로 빌드하여 `web/pkg/`에 배치 (`staveloom_wasm_bg.wasm`, 1.1 MB).
- `parse_and_render()` WASM 함수: MusicXML bytes → `{ systems, metadata, midi }` 반환.
- 파일 업로드(드래그앤드롭 / 파일 선택) → WASM 파싱 → SVG 렌더링 → 다운로드.

#### Phase 2 — 클라이언트 사이드 MIDI 합성 (완료)

- **SpessaSynth** (`lib/spessasynth_lib.min.js` + `spessasynth_processor.min.js`): AudioWorklet 기반 실시간 SF2 합성.
- **On-demand SoundFont 로딩**: `soundfonts/index.json`을 참조하여 악기별 SF2 파일을 필요할 때만 fetch. 악기당 1~5 MB.
- **SF2 병합 전략**: 여러 SF2를 `mergeSF2Buffers()`로 단일 SF2로 병합 후 `addSoundBank()` 1회 호출 → 채널 리셋 방지.
- **songChange 이벤트 대기**: `loadNewSongList()`는 AudioWorklet postMessage 비동기. `'songChange'` 이벤트 후에야 `duration` / `currentTime` 유효.
- 실시간 커서 동기화: `currentHighResolutionTime` + `audioLatency` 보정으로 sub-frame 정밀도.

#### Phase 3 — 가상 SVG 렌더링 (완료)

시스템(보표 행) 수에 따른 3-tier 렌더링 전략:

| 조건                              | 전략                                |
| --------------------------------- | ----------------------------------- |
| 시스템 ≤ 10 (`COMBINE_THRESHOLD`) | 단일 SVG 결합 (`_combineSystems()`) |
| 시스템 > 10                       | IntersectionObserver 가상화         |

- **`_combineSystems()`**: N개 per-system SVG를 `viewBox="0 0 W totalH"` 하나로 병합. `<g id="sN">` 그룹으로 시스템 분리.
- **IntersectionObserver 가상화**: 뷰포트에 들어오는 시스템만 SVG를 마운트. rootMargin ≈ 2 시스템 높이로 동적 계산하여 선행 렌더링.
- **`processFile(filename, bytes, fullReset)`**: `fullReset=true` 시에만 스크롤 리셋·MIDI 리셋 수행. 레이아웃 토글 재렌더링 시 스크롤 위치 보존.

#### Phase 4 — PWA 오프라인 지원 (완료)

- **Service Worker** (`web/sw.js`, `CACHE_VERSION='v2'`): Cache-First + 동적 캐싱. `ignoreSearch: true`로 `?v=N` 버전 쿼리 무시. `Promise.allSettled`로 부분 캐시 실패 허용.
- **사전 캐시 목록 (16개)**: index.html, style.css, player.js, virtual-score.js, soundfont-loader.js, midi-player.js, manifest.json, icon-192.png, icon-512.png, staveloom_wasm.js, staveloom_wasm_bg.wasm, spessasynth_lib.min.js, spessasynth_processor.min.js, soundfonts/index.json, fonts/Bravura-subset.woff2.
- **Service-Worker-Allowed: /** 헤더: `server.py`에서 sw.js 응답 시 추가. SW 스코프를 사이트 루트(`/`) 전체로 허용.
- **PWA Manifest** (`web/manifest.json`): `display: standalone`, `theme_color: #6c5ce7`, 192·512px 아이콘.

#### Bravura 폰트 번들링 (완료)

- 시스템에 Bravura가 없는 환경 대응: OTF(500.9 KB, 3,693 glyph) → WOFF2 subset (28.3 KB, 212 glyph).
- `@font-face { src: local('Bravura'), url('./fonts/Bravura-subset.woff2') }`: 로컬 설치가 있으면 우선 사용, 없으면 URL 폴백.
- `<link rel="preload">` + `font-display: block`: 첫 SVG 렌더 전 폰트 로드 보장, FOUT 방지.
- 생성 스크립트: `scripts/generate_bravura_subset.sh` (`pyftsubset` 사용).

#### 모바일 UX (완료)

- **사이드바 토글**: 모바일(≤768px) 기본 숨김. 좌상단 햄버거(☰/✕) 버튼으로 오버레이 슬라이드 인. 백드롭 탭 또는 파일 선택 시 자동 닫힘.
- **iOS 오디오 언락**:
  - `unlockAudio()`: 사용자 제스처 동기 콜스택에서 `AudioContext` 생성 + `resume()` 호출. `FileReader.onload` 비동기 콜백 진입 전 반드시 선행 호출.
  - `play()` async화: `await this._ctx.resume()` 후 `seq.play()` 호출. Suspended 컨텍스트에서 재생 시 무음 방지.
  - `touchstart` (capture, passive) 리스너: 화면 터치마다 AudioContext 재개 시도. 백그라운드 전환 후 iOS 재-suspension 대응.
  - `visibilitychange` 리스너: 탭 복귀 시 `unlockAudio()` 시도.
- **2줄 툴바**: 모바일(≤768px)에서 `.controller-inner { flex-wrap: wrap }`. 1줄: 시간 + 재생 버튼. 2줄: 볼륨/줌/다운로드(`space-around`). 구분선 숨김.
- **Safe Area 지원**: `viewport-fit=cover` + `env(safe-area-inset-bottom)` + `100dvh`. iOS Safari URL bar 동적 대응 및 홈 인디케이터 위에 툴바 배치.

### 6. 검증 및 리포팅 시스템

- **스냅샷 테스트**: JSON/SVG/Timeline/Metadata 다각도 회귀 테스트 (100% 통과).
- **시각적 리포트**: W3C 표준 샘플 + 복합 주법 샘플 → `specification_report.html` 자동 생성.
- **메타데이터 추출**: `--metadata <file.json>` CLI 플래그로 박자별 SVG 좌표·절대 시간 JSON 추출.
- **대화형 미리보기**: `preview.html` (전체 SVG 탐색), `preview_debug.html` (동기화 라인 시각화).

---

## 주요 기술 스택

- **Core**: Rust (Edition 2024) — roxmltree, serde, svg, midly, rustysynth, lame
- **WASM**: wasm-pack, wasm-bindgen (wasm32-unknown-unknown 타겟)
- **Web Player**: Vanilla JS (ES2022 modules), SpessaSynth (AudioWorklet), Service Worker
- **Font**: SMuFL / Bravura (WOFF2 subset, fonttools pyftsubset)
- **Standard**: MusicXML 3.1/4.0
- **Dev Server**: Python 3 (표준 라이브러리) — WASM MIME, `Service-Worker-Allowed` 헤더

---

## 파일 구조 (주요)

```
staveloom/
├── crates/
│   ├── staveloom-core/   — Rust 렌더링·파싱·MIDI 엔진
│   ├── staveloom-cli/    — CLI 바이너리
│   └── staveloom-wasm/   — wasm-bindgen WASM 바인딩
├── web/
│   ├── pkg/              — wasm-pack 빌드 결과물 (staveloom_wasm_bg.wasm)
│   ├── lib/              — SpessaSynth 번들
│   ├── fonts/            — Bravura-subset.woff2
│   ├── player.js         — 메인 플레이어 로직
│   ├── midi-player.js    — MidiPlayerController + SF2 병합
│   ├── virtual-score.js  — VirtualScoreRenderer (IntersectionObserver)
│   ├── soundfont-loader.js — 온디맨드 SF2 로더
│   ├── sw.js             — Service Worker (PWA)
│   └── manifest.json     — PWA 매니페스트
├── scripts/
│   ├── generate_bravura_subset.sh — Bravura WOFF2 subset 생성
│   └── ...               — 리포트·프리뷰·샘플 스크립트
├── docs/
│   ├── phase1~4_walkthrough.md — 각 Phase 구현 상세
│   └── WEB_PLAYER.md     — 웹 플레이어 전체 문서
├── server.py             — 개발용 HTTP 서버
└── wasm-build.sh         — WASM 빌드 스크립트
```

---

## 개발 명령어 참조

```bash
# WASM 빌드
bash wasm-build.sh

# 웹 플레이어 개발 서버
python3 server.py  # → http://localhost:8000

# Rust CLI 빌드 및 실행
cargo build --release
cargo run -- <file.musicxml> --output out.svg

# Bravura 폰트 subset 재생성 (fonttools 필요)
bash scripts/generate_bravura_subset.sh

# 전체 테스트
cargo test

# 시각적 리포트
python3 scripts/generate_report.py
```
