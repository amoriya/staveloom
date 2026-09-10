# Staveloom — 기술 아키텍처 문서

## 목차

1. [전체 구조 개요](#1-전체-구조-개요)
2. [Rust 코어 라이브러리 (staveloom-core)](#2-rust-코어-라이브러리-staveloom-core)
   - 2.1 [파서 (parser.rs)](#21-파서-parserrs)
   - 2.2 [데이터 모델 (models.rs)](#22-데이터-모델-modelsrs)
   - 2.3 [렌더러 (renderer/)](#23-렌더러-renderer)
   - 2.4 [타임라인 솔버 (timeline.rs)](#24-타임라인-솔버-timeliners)
   - 2.5 [MIDI 엔진 (midi_engine.rs)](#25-midi-엔진-midi_enginers)
   - 2.6 [MIDI 역파서 (midi_parser/)](#26-midi-역파서-midi_parser)
3. [CLI 바이너리 (staveloom-cli)](#3-cli-바이너리-staveloom-cli)
4. [WASM 바인딩 (staveloom-wasm)](#4-wasm-바인딩-staveloom-wasm)
5. [웹 플레이어 (web/)](#5-웹-플레이어-web)
   - 5.1 [player.js — 메인 오케스트레이터](#51-playerjs--메인-오케스트레이터)
   - 5.2 [virtual-score.js — 가상 SVG 렌더러](#52-virtual-scorejs--가상-svg-렌더러)
   - 5.3 [midi-player.js — 오디오 엔진](#53-midi-playerjs--오디오-엔진)
   - 5.4 [soundfont-loader.js — SF2 로더](#54-soundfont-loaderjs--sf2-로더)
   - 5.5 [sw.js — Service Worker (PWA)](#55-swjs--service-worker-pwa)
6. [데이터 플로우](#6-데이터-플로우)
   - 6.1 [웹 플레이어 데이터 플로우](#61-웹-플레이어-데이터-플로우)
   - 6.2 [CLI 데이터 플로우](#62-cli-데이터-플로우)
7. [핵심 알고리즘](#7-핵심-알고리즘)
8. [배포 및 빌드](#8-배포-및-빌드)
9. [의존성 트리](#9-의존성-트리)

---

## 1. 전체 구조 개요

Staveloom은 단일 Cargo 워크스페이스 안에 3개의 크레이트와 하나의 독립적인 웹 프런트엔드로 구성됩니다.

```
staveloom/                          ← Cargo workspace root
├── crates/
│   ├── staveloom-core/             ← 핵심 라이브러리 (파서, 모델, 렌더러, MIDI 엔진)
│   ├── staveloom-cli/              ← CLI 바이너리 (staveloom-core 소비자)
│   └── staveloom-wasm/             ← WASM 바인딩 (staveloom-core 소비자)
├── web/                       ← 순수 클라이언트 사이드 웹 플레이어
│   ├── pkg/                   ← wasm-pack 빌드 결과물 (git 미추적, 빌드 시 생성)
│   ├── lib/                   ← SpessaSynth 번들
│   ├── fonts/                 ← Bravura WOFF2 subset
│   └── soundfonts/            ← 악기별 SF2 파일
├── server.py                  ← 개발용 정적 파일 서버
└── wasm-build.sh              ← WASM 빌드 스크립트
```

**핵심 원칙**: staveloom-core는 순수 라이브러리로, I/O 없이 메모리 내 데이터만 다룹니다. CLI와 WASM은 각각 staveloom-core를 호출하는 얇은 진입점입니다. 웹 플레이어는 서버를 전혀 사용하지 않으며, 렌더링·합성·재생이 모두 브라우저 안에서 완결됩니다.

---

## 2. Rust 코어 라이브러리 (staveloom-core)

```
crates/staveloom-core/src/
├── lib.rs               — 공개 모듈 선언
├── models.rs            — 전체 데이터 모델 (Score ~ Note)
├── parser.rs            — MusicXML → Score 파서
├── auto_beam.rs         — 빔(beam) 정보가 없는 음표에 자동으로 beam 부여
├── timeline.rs          — 연주 순서 해석기 (반복·점프)
├── midi_engine.rs       — MIDI 시퀀서 + 타이밍 맵 (Score → SMF)
├── midi_parser/         — MIDI 파일 → Score 역파서 (아래 2.6 참고)
│   ├── mod.rs           — MidiParser 공개 API
│   ├── event.rs         — SMF 이벤트 → RawNote/TempoChange 등 1차 추출
│   ├── quantizer.rs     — 틱 → 음표 길이 양자화, 휴먼 연주 감지
│   ├── builder.rs       — 양자화된 이벤트 → Score 조립 (2,300줄+)
│   └── pitch.rs         — MIDI pitch → 음이름/음자리표/타악기 표기 변환
├── instrument_maps.rs   — 악기 이름 → MIDI program 번호 매핑
└── renderer/
    ├── mod.rs           — Renderer 구조체 + draw_* 메서드 전체 (음표, 가사, 코드, 다이나믹스,
    │                       속성, 슬러/타이/스패너 등 모든 기호 렌더링 포함, 11,000줄+)
    ├── layout.rs        — 마디 너비 분석 (analyze_measures)
    ├── types.rs         — SpacingStrategy, OnsetNeeds 등
    └── utils.rs         — SMuFL 코드포인트, 보조 함수
```

### 2.1 파서 (parser.rs)

**역할**: MusicXML/MXL 바이트 → `Score` 구조체

| 함수                     | 설명                                            |
| ------------------------ | ----------------------------------------------- |
| `load_and_parse(path)`   | 파일 경로 → 파싱 (CLI 용; `.mxl` zip 자동 해제) |
| `parse_in_memory(bytes)` | 바이트 슬라이스 → 파싱 (WASM/테스트 용)         |

**파싱 전략**:

- `roxmltree`로 XML DOM 트리 생성 (event-driven이 아닌 tree 방식 — `find()` 기반 전방 탐색 가능)
- `.mxl` 감지: 파일 시작 바이트 `PK\x03\x04` (ZIP magic) → `META-INF/container.xml` → 루트 파일 경로 추출; 실패 시 첫 번째 `.xml` 항목으로 폴백
- `<part-list>` → `PartListItem` 목록 (악기 메타데이터, MIDI 프로그램, instrument-sound)
- `<part>/<measure>` → `Part::measures: Vec<Measure>`
- `<note>`, `<backup>`, `<forward>`, `<direction>`, `<harmony>` 등 → `MeasureElement` enum 배리언트
- 비표준 구조 대응: `<technical>`이 `<notations>` 밖에 있는 경우 등 재귀 탐색으로 처리

### 2.2 데이터 모델 (models.rs)

전체 악보를 표현하는 Rust 타입 계층:

```
Score
└── Vec<Part>
    └── Vec<Measure>
        ├── attributes: Option<Attributes>  ← 박자표, 조표, 음자리표, divisions
        └── elements: Vec<MeasureElement>
            ├── Note          ← 음표/쉼표 (pitch, duration, notations, stem, beam, ...)
            ├── Backup(i32)   ← 다성부 역방향 이동
            ├── Forward(i32)  ← 다성부 전방 이동
            ├── Direction     ← 다이나믹스, 텍스트, 페달, 오타바 등
            ├── Attributes    ← 마디 중간 음자리표/조표/박자표 변경
            ├── Sound         ← 템포 변경
            ├── Barline       ← 마디줄 (repeat, ending, bar-style)
            ├── Frame         ← 코드 다이어그램 (fret diagram)
            ├── Harmony       ← 코드 표기
            ├── Bookmark(String) ← 앵커 id
            ├── FiguredBass   ← 통주 저음
            └── Grouping      ← 음표 묶음 브래킷
```

**출력 타입** (`PlayMetadata`):

```rust
pub struct PlayMetadata {
    pub beats: Vec<BeatEvent>,           // 박자 동기화 포인트
    pub system_boundaries: Vec<SystemBoundary>, // 시스템 레이아웃 정보
}

pub struct BeatEvent {
    pub x: f32,            // SVG 좌표 (px)
    pub y_start: f32,
    pub y_end: f32,
    pub measure_index: usize,
    pub measure_number: String,
    pub beat_number: f32,     // 1.0, 2.0, 3.0 ...
    pub absolute_beat: f32,   // 곡 전체 누적 박자 카운터
    pub time_seconds: f32,    // 절대 시간 (초)
    pub system_index: usize,
}

pub struct SystemBoundary {
    pub y_start: f32,
    pub height: f32,
    pub measure_start: usize,
    pub measure_end: usize,
}
```

### 2.3 렌더러 (renderer/)

**진입점**: `Renderer::render_with_metadata(score) → (String, PlayMetadata)`

```rust
pub struct Renderer {
    pub staff_line_distance: f32,  // 보표 간격 (기본 10.0px)
    pub page_width: Option<f32>,   // None = 가로 한 줄 모드
    pub spacing_strategy: SpacingStrategy,  // Elastic | Compact | Mobile
    // ... margin, font_family, etc.
}
```

**렌더링 파이프라인**:

```
1. analyze_measures()  [layout.rs]
   ├── 각 마디별 OnsetNeeds 계산 (음표 머리, 임시표, 꾸밈음 공간)
   ├── SpacingStrategy::Elastic → W = C × duration^0.6 비선형 공식
   ├── SpacingStrategy::Compact → 실제 렌더링 너비 기반, 고정 12px 온셋 간격
   └── SpacingStrategy::Mobile → Compact와 유사하되 더 좁은 고정 온셋 간격 (좁은 화면용)

2. system_break 계산
   └── 누적 마디 너비 > page_width 시 새 시스템 시작

3. 시스템별 렌더링
   ├── draw_attributes()        — 음자리표, 조표, 박자표
   ├── draw_chord_group_at()    — 음표 머리, 임시표, 기둥, 보, 점 (draw_notehead 등 호출)
   ├── draw_direction()         — 다이나믹스, 텍스트, 페달, 오타바
   ├── render_harmony()         — 코드 표기
   ├── draw_lyric()             — 가사
   └── draw_slur() / draw_tie() / draw_wedge() / draw_octave_shift() 등 — 슬러, 타이, 크레셴도, 옥타브 라인 등

4. 우선순위 기반 수직 스태킹 충돌 방지 (§7 참고)
   └── 11단계 우선순위 (Dynamics → ... → Harmony → Lyrics → ...)

5. 메타데이터 수집
   └── 각 BeatEvent { x, y_start, y_end, measure_index, beat_number, time_seconds, ... } 기록
```

**SVG 구조**:

```xml
<svg viewBox="0 0 W totalH">
  <style>@font-face { ... }</style>
  <g id="s0">  <!-- 시스템 0 -->
    <line/>  <!-- 보표 선 5개 -->
    <text>𝄞</text>  <!-- SMuFL 기호 (Bravura) -->
    <path/>  <!-- 기둥, 보, 슬러 등 -->
  </g>
  <g id="s1">  <!-- 시스템 1 -->
    ...
  </g>
</svg>
```

### 2.4 타임라인 솔버 (timeline.rs)

**역할**: MusicXML의 연주 기호를 해석하여 실제 연주 마디 순서를 `Vec<usize>` (마디 인덱스 배열)로 반환.

```rust
pub struct TimelineSolver;
impl TimelineSolver {
    pub fn solve(score: &Score) -> Vec<usize>
}
```

**처리 기호**:

- `<repeat direction="forward/backward">` — 도돌이표
- `<ending number="1,2">` — Volta bracket (1번, 2번 괄호)
- `D.S.` / `D.C.` — Da Segno / Da Capo (텍스트 파싱)
- `<segno>` / `<coda>` — 기호 위치 사전 스캔
- `implicit="yes"` — 못갖춘마디 처리

**알고리즘**: 단일 패스 반복 시뮬레이션 + `repeat_counts: HashMap<usize, usize>`로 반복 횟수 추적. 최대 종료 번호(Ending)를 기준으로 반복 탈출 조건 결정.

### 2.5 MIDI 엔진 (midi_engine.rs)

```rust
pub struct MidiEngine;
impl MidiEngine {
    pub fn generate_smf(score: &Score, timeline: &[usize]) -> Smf      // MIDI 파일 생성
    pub fn generate_timing_map(score, timeline) -> Vec<TimingEvent>     // 절대 시간 맵
    pub fn get_program_number(sound, midi, name, names) -> i32          // 악기 → GM program
}
```

**MIDI 생성 파이프라인**:

```
timeline (마디 순서) + Score
  ├── generate_timing_map() → 마디별 절대 시간(초), 템포, 박자
  ├── 각 Part → MIDI Track
  │   ├── 프로그램 번호 결정 (instrument_sound → MIDI program)
  │   ├── Note → NoteOn / NoteOff 이벤트 (tick 단위)
  │   ├── 장식음 전개: Trill, Mordent, Arpeggio, Tremolo → 음표 시퀀스로 확장
  │   └── dynamic_to_velocity(): 다이나믹스 표기(ppp~fff) → MIDI velocity(40~127) 매핑
  └── Smf (midly crate) → SMF Format 1 바이너리
```

**참고**: articulation(staccato/tenuto/accent 등)은 현재 MIDI 생성(재생음)에는 반영되지
않고, 렌더러(`renderer/mod.rs`)에서 악보 위 기호(SMuFL glyph)로만 그려집니다.

### 2.6 MIDI 역파서 (midi_parser/)

**역할**: 표준 MIDI 파일(SMF) → `Score` 구조체. `midi_engine.rs`(Score → MIDI)의 반대
방향으로, 사용자가 MusicXML이 아닌 `.mid` 파일을 직접 드래그해도 악보로 렌더링할 수
있게 해주는 모듈입니다 (웹 플레이어의 MIDI 파일 업로드 기능, §6.1 참고). 순수 재생용
`generate_timing_map()`과 달리, 여기서는 사람이 연주한(비양자화) MIDI도 처리해야 하므로
로직이 훨씬 큽니다 (5개 파일 합계 5,200줄+, `parser.rs`의 MusicXML 파서보다 큼).

```rust
pub struct MidiParser;
impl MidiParser {
    pub fn parse(bytes: &[u8]) -> Result<Score, MidiParseError>
    pub fn parse_events(bytes: &[u8]) -> Result<ParsedMidi, MidiParseError>
}
```

**파이프라인**:

```
1. event.rs   — SMF 트랙 이벤트 → RawNote(NoteOn/NoteOff 매칭) + TempoChange + KeySigChange + TimeSigChange
2. quantizer.rs
   ├── detect_grid() — 곡 전체 노트 시작 시각 분포로 최적 격자(그리드) 추정
   ├── is_human_midi() — 사람 연주(타이밍 흔들림) 여부 판정
   └── quantize_note() / decompose_duration() — 틱을 표준 음표 길이로 스냅 + 타이로 연결된
       복합 길이 분해
3. pitch.rs   — MIDI note number → 음이름/옥타브/조표에 맞는 enharmonic 선택,
                채널 10(타악기)은 drum_notehead()/drum_position()으로 별도 표기
4. builder.rs — 위 결과를 Note/Measure/Part/PartListItem으로 조립 → Score 반환
```

**사용처**: `staveloom-wasm`의 `list_midi_parts()` / `parse_midi_and_render()`에서만
호출됩니다 — `staveloom-cli`는 MusicXML/.mxl 입력만 지원하며 MIDI 파일 입력 경로가 없습니다.

---

## 3. CLI 바이너리 (staveloom-cli)

```
crates/staveloom-cli/src/
├── main.rs         — 인수 파싱, 파이프라인 오케스트레이션
└── audio_render.rs — rustysynth (SF2 합성) + LAME (MP3 인코딩)
```

**주요 플래그**: `--output`, `--metadata`, `--audio`, `--midi`, `--sf2`, `--json`,
`--list-parts`, `--parts`, `--width`, `--horizontal`, `--elastic`, `--mobile`
(§2.3 `SpacingStrategy::Mobile` 적용), `--license` (서드파티 라이선스 고지 출력).
MIDI 파일 입력은 지원하지 않음 — MusicXML/.mxl 전용 (`midi_parser`는 `staveloom-wasm`에서만 사용, §2.6 참고).

**처리 흐름**:

```
CLI 인수
  → load_and_parse(path)         [staveloom-core::parser]
  → score.filter_parts()         [선택적]
  → Renderer::render()           → SVG 파일 저장
  → TimelineSolver::solve()      → Vec<usize>
  → MidiEngine::generate_smf()   → MIDI 파일 저장
  → audio_render::render_mp3()   → rustysynth PCM → LAME → MP3
```

**오디오 렌더링** (`audio_render.rs`):

- `rustysynth::SynthesizerSettings` + SF2 로드
- MIDI 데이터를 `MidiFileSequencer`로 재생하여 `f32` PCM 샘플 생성
- `lame` 크레이트(`lame::Lame`, 시스템 `libmp3lame` 동적 링크)로 PCM → MP3 인코딩

---

## 4. WASM 바인딩 (staveloom-wasm)

**빌드**: `wasm-pack build --target web` → `web/pkg/` 산출물

**공개 API** (JavaScript에서 호출 가능):

| 함수                                                                                | 입력                | 출력                                        |
| ----------------------------------------------------------------------------------- | ------------------- | ------------------------------------------- |
| `list_parts(bytes)`                                                                 | `Uint8Array`        | `PartInfo[]` (`{id, name}`)                 |
| `list_instruments(bytes)`                                                           | `Uint8Array`        | `InstrumentInfo[]` (`{program, name}`)      |
| `list_midi_parts(bytes)`                                                            | `Uint8Array`        | `PartInfo[]` (`{id, name}`, MIDI 트랙 기반) |
| `parse_and_render(bytes, elastic, mobile, horizontal, page_width, filter_csv)`      | `Uint8Array` + 옵션 | `RenderResult`                              |
| `parse_midi_and_render(bytes, elastic, mobile, horizontal, page_width, filter_csv)` | `Uint8Array` + 옵션 | `RenderResult` (MIDI 입력용)                |

**`parse_and_render` 내부 처리**:

```
parse_in_memory(bytes) → Score
score.filter_parts()   [filter_csv가 있을 때]
Renderer::render_with_metadata() → (svg_string, PlayMetadata)

if n_systems <= 1:
  SystemSvg { svg_content: 전체 SVG }

else:
  metadata.system_boundaries.iter().map(|boundary| {
    inner = extract_system_group(svg, i)   // <g id="sN"> 내용 추출
    sys_svg = format!(<svg viewBox="0 {y_start} W {h}">...)  // 시스템별 SVG
    SystemSvg { svg_content: sys_svg, y_offset, height, ... }
  })

TimelineSolver::solve() → Vec<usize>
MidiEngine::generate_smf() → SMF bytes

RenderResult { systems, metadata, midi }
→ serde_wasm_bindgen::to_value() → JsValue
```

**직렬화**: `serde_wasm_bindgen`으로 Rust 구조체를 JS 객체로 변환. `PlayMetadata`는 `serde::Serialize`를 구현하며 JSON 직렬화 없이 JS 객체로 직접 변환.

---

## 5. 웹 플레이어 (web/)

완전한 클라이언트 사이드 아키텍처. HTTP 서버는 정적 파일 제공만 담당.

```
web/
├── index.html            — 진입점, 레이아웃, 스크립트 로드
├── style.css             — UI 스타일 (다크 테마, 글래스모피즘)
├── player.js             — 메인 오케스트레이터
├── virtual-score.js      — VirtualScoreRenderer
├── midi-player.js        — MidiPlayerController + SF2 병합
├── soundfont-loader.js   — SoundFontLoader
├── sw.js                 — Service Worker
├── manifest.json         — PWA 매니페스트
├── icon-192.png / icon-512.png — PWA 아이콘
├── legal/                — LICENSE-MIT, LICENSE-APACHE, THIRDPARTY_LICENSE.md
│                            (docs/THIRDPARTY_LICENSE.md와 동일 사본, deploy 시 web/만 배포되므로 필요)
├── pkg/                  — wasm-pack 빌드 결과물
│   ├── staveloom_wasm.js      — WASM JS glue (9.9 KB)
│   └── staveloom_wasm_bg.wasm — WASM 바이너리 (1.1 MB)
├── lib/
│   ├── spessasynth_lib.min.js       — SpessaSynth 라이브러리 (259 KB)
│   └── spessasynth_processor.min.js — AudioWorklet 프로세서 (387 KB)
├── fonts/
│   └── Bravura-subset.woff2 — SMuFL 폰트 subset (28.3 KB, 212 glyph)
└── soundfonts/            — 악기별 SF2 파일 (index.json + instruments/*.sf2, 129개)
```

### 5.1 player.js — 메인 오케스트레이터

**구조**: 단일 async IIFE 안에 상태와 핸들러 전체를 캡슐화.

```javascript
// 상태
state = {
  filename, rawFileBytes, midiBytes, svgContent, systems, metadata,
  instruments, parts, currentZoom, isPlaying, isScrubbing,
  currentSystemIndex, currentLineY,
  mobileUserOverridden, preMobileWidthValue,  // 모바일 자동 감지용
}

// 초기화 순서 (DOMContentLoaded)
init()
  ├── VirtualScoreRenderer 생성
  ├── initWasm() + midiPlayer.init() 동시 시작 (Promise.all로 마지막에 대기)
  ├── applyAutoMobileDetect(true) + resize 리스너 등록 — 모바일 레이아웃 자동 감지
  ├── setupDragAndDrop()
  ├── setupControls()
  ├── setupKeyboardShortcuts()
  ├── setupMidiPlayerCallbacks()
  ├── setupMobileAudio()    — touchstart / visibilitychange 핸들러
  └── await Promise.all([initWasm(), midiPlayer.init()])
```

**모바일 레이아웃 자동 감지** (`applyAutoMobileDetect`):

- 초기 로드 및 `resize` 이벤트마다 `window.innerWidth <= 480`(`MOBILE_AUTO_BREAKPOINT`)이면
  `mobileToggle.checked = true`로 설정 → WASM 렌더 호출 시 `mobile=true`가 전달되어
  `SpacingStrategy::Mobile` 적용 (§2.3)
- 사용자가 모바일 토글을 직접 클릭하면 `state.mobileUserOverridden = true`로 표시되어
  이후 자동 감지(리사이즈 포함)가 멈추고 사용자 선택이 우선
- 모바일 on 전환 시 `widthSlider` 값을 `max(320, min(innerWidth, 500))`로 스냅하고 이전 값을
  `state.preMobileWidthValue`에 저장; off 전환 시 그 값으로 복원
- 모바일이 켜져 있는 동안 elastic 토글은 비활성화 (`SpacingStrategy::Mobile`이 우선)

**파일 처리 파이프라인** (`processFile`):

```javascript
processFile(filename, bytes, fullReset=true)
  1. 재생 중이면 pause()
  2. isMidiFile(bytes) — 파일 시작 바이트로 SMF("MThd") 여부 판별
     ├── MIDI 파일: WASM list_midi_parts() + parse_midi_and_render()
     │              (MIDI → Score 역파싱, midi_parser 모듈 경유)
     └── MusicXML/.mxl: WASM list_parts() + list_instruments() + parse_and_render()
  3. 위 결과 → { systems, metadata, midi }
  4. fullReset 시: 상태 갱신, 파트 목록 UI 업데이트
  5. VirtualScoreRenderer.load(systems)  — SVG 마운트
  6. fullReset 시: requestAnimationFrame(() => scroll reset)
  7. 타임라인 UI 초기화
  8. [비동기] midiPlayer.loadMidi(midi, instruments)
     └── SF2 로드 → SpessaSynth 초기화 → Sequencer 준비
```

MIDI 파일 입력 시에도 `midi` 필드는 원본 SMF 바이트가 아니라, `MidiParser::parse()`가
역파싱한 `Score`를 다시 `MidiEngine::generate_smf()`로 재생성한 바이트입니다 — 양자화된
`Score`와 재생 타이밍(BeatEvent)이 항상 정확히 일치하도록 하기 위함입니다.

**재생 루프** (requestAnimationFrame):

```javascript
startPlaybackLoop()
  └── loop():
      ├── updateTimelineScrubber()  — 스크러버 위치 + 시간 표시
      ├── updateCursorPosition()    — BeatEvent 이진 탐색 → 커서 SVG 좌표
      └── autoScroll()              — 시스템 경계 넘어갈 때 스크롤
```

**커서 위치 계산** (`getCursorPosition`):

- `beats[]`에서 `time_seconds <= currentTime`인 마지막 인덱스 이진 탐색
- 같은 보표 행 내에서 beatA↔beatB 선형 보간 → 부드러운 커서 이동
- 시스템 경계 넘어갈 때: `ratio < 0.85` → beatA 위치, `>= 0.85` → beatB 위치로 스냅

### 5.2 virtual-score.js — 가상 SVG 렌더러

```javascript
class VirtualScoreRenderer {
  constructor(container, scrollRoot)

  loadSingle(svgContent)        // 단일 SVG (≤10 시스템은 병합 후 이 경로로)
  load(systems)                 // 시스템 배열 → 전략 선택 (>10이면 가상화 직접 설정)
  _combineSystems(systems)      // ≤10 시스템: 단일 SVG 병합
  _mountSystem(idx) / _unmountSystem(idx) // >10 시스템: IntersectionObserver 콜백
  updateCursor(systemIdx, x, y1, y2)
  hideCursor()
  scrollToSystem(idx)
  scrollHorizontalToCursor()
}
```

**렌더링 전략** (`load(systems)` 내부에서 시스템 수로 분기 — 1개 시스템은 별도 케이스가
아니라 자연스럽게 "≤ COMBINE_THRESHOLD" 경로를 탐):

```
systems.length <= COMBINE_THRESHOLD (10)
  → _combineSystems() 후 loadSingle():
      1. 각 시스템 SVG에서 <style> 추출 (첫 시스템 것만 사용)
      2. 각 시스템 SVG에서 <g id="sN"> 내용 추출 (_extractGroupContent, depth-counting 파서)
      3. viewBox="0 0 W totalH" 로 단일 SVG 조립
      4. 전체를 #score-canvas에 직접 innerHTML로 마운트

systems.length > COMBINE_THRESHOLD
  → load()가 직접 가상화 설정:
      1. 각 시스템마다 .system-wrapper div 생성 (aspect-ratio = width/height, 실제 높이는
         레이아웃 시점에 브라우저가 계산 — 텍스트 없이도 스크롤 위치가 안정적)
      2. div를 #score-canvas에 append (DOM에 있지만 SVG는 없음)
      3. IntersectionObserver 설정:
         rootMargin = "±max(뷰포트 폭 기준 시스템 평균 높이 × 2, 120)px 0px"
      4. 뷰포트 진입 → _mountSystem(idx): div.innerHTML = system.svg_content
         뷰포트 이탈 → _unmountSystem(idx): div.innerHTML = '' (언마운트, 메모리 해제)
```

**커서 렌더링**:

- 대상 시스템의 SVG 내부에 `<line id="playback-cursor">` 동적 추가/이동
- CSS: `stroke: var(--accent-danger)`, `filter: drop-shadow(...)`, `pointer-events: none`
- 시스템 전환 시 이전 시스템의 커서 제거 → 새 시스템에 추가

### 5.3 midi-player.js — 오디오 엔진

```javascript
class MidiPlayerController {
  _ctx: AudioContext       // Web Audio API
  _synth: WorkletSynthesizer  // SpessaSynth
  _gainNode: GainNode
  _seq: Sequencer
  _sfLoader: SoundFontLoader
  _loadedSoundFonts: Set   // 중복 로드 방지 (ArrayBuffer 단위)
  onEnded: Function | null // player.js가 재생 종료 콜백으로 설정

  // 공개 API
  async init()
  unlockAudio()            // iOS 제스처 내 동기 호출 (AudioContext 생성/resume)
  async loadMidi(bytes, instruments)
  async play()             // await resume() → seq.play()
  pause()
  get currentTime()        // currentHighResolutionTime (sub-frame 정밀도)
  get duration()
  get audioLatency()       // outputLatency + baseLatency
  get isPlaying()
  get isReady()             // AudioContext 생성 여부 (사용자 제스처 발생했는지)
  setVolume(vol)
  setMuted(muted)
}
```

**AudioContext 초기화 전략** (iOS Safari 대응):

```
사용자 제스처 (click/change/touchstart)
  └── unlockAudio()  ← 반드시 동기 콜스택 안에서 호출
      ├── new (window.AudioContext || window.webkitAudioContext)()
      ├── gainNode 연결
      └── ctx.resume()  ← fire-and-forget (gesture 컨텍스트에서 실행)

비동기 콜백 (FileReader.onload, loadMidi 내부)
  └── _ensureAudio()
      ├── unlockAudio() 미호출 시 폴백 생성 (Android/Desktop만 실제 작동)
      ├── await ctx.resume()  ← suspended 상태 대기
      └── AudioWorklet + WorkletSynthesizer 초기화 (최초 1회)
```

**SF2 병합 전략** (`mergeSF2Buffers`):

```
악기별 ArrayBuffer[] (각각 1~5 MB SF2)
  └── mergeSF2Buffers()
      ├── 각 SF2 파싱: smpl(PCM), phdr, pbag, pmod, pgen, inst, ibag, imod, igen, shdr
      ├── smpl 블록 순차 연결 + 각 shdr의 sample offset 재계산
      ├── phdr/inst의 bag 인덱스 조정
      ├── pbag/ibag의 gen/mod 인덱스 조정
      ├── pgen의 instrument reference, igen의 sampleID reference 조정
      └── 단일 RIFF sfbk 바이너리 조립

→ 병합된 SF2 1개로 addSoundBank() 1회 호출
   (N회 호출 시 채널 리셋 N번 발생 → 다중 악기 재생 불가)
```

**`songChange` 이벤트 처리**:

```javascript
// loadNewSongList()는 AudioWorklet에 postMessage를 보내고 즉시 반환
// midiData/duration은 Worklet 응답 후 'songChange' 이벤트에서만 유효
const midiReady = new Promise((resolve) => {
  seq.eventHandler.addEvent("songChange", "staveloom-player-load", resolve);
});
seq.loadNewSongList([{ binary: midiBuffer }]);
await midiReady; // Worklet 응답 대기
```

### 5.4 soundfont-loader.js — SF2 로더

```javascript
class SoundFontLoader {
  _memCache: Map<string, ArrayBuffer>   // 메모리 캐시 (파일명 → ArrayBuffer)
  _db: IDBDatabase | null               // IndexedDB
  _index: object | null                 // index.json 파싱 결과
  _dbName: string                       // 'staveloom-soundfonts-v1'
  _storeName: string                    // 'sf2-cache'

  async init()
    ├── fetch('./soundfonts/index.json') → _index 구성
    └── indexedDB.open(_dbName) → _db 연결  (병렬 실행)

  async loadForPrograms(programs[])  // → [{ program, data: ArrayBuffer }]
    ├── program 번호 중복 제거 후, 각 program → _index.instruments[program].file 조회
    ├── 파일명 기준 중복 제거 (여러 program이 같은 파일을 가리켜도 fetch 1회)
    └── 각 파일 (_loadFile):
        1. 메모리 캐시 (_memCache) 조회 → 히트 시 즉시 반환
        2. IndexedDB (_db) 조회 → 히트 시 메모리 캐시에 저장 후 반환
        3. fetch 네트워크 요청 → 메모리 캐시 저장 → IDB 저장(fire-and-forget) → 반환
           (fetch 실패 시 _index.fallback 파일로 재시도)
}
```

**index.json 구조** (실제로는 악기별 1:1 매핑 — program별 range 그룹핑은 없음):

```json
{
  "instruments": {
    "0": {
      "file": "000_acoustic_grand_piano.sf2",
      "name": "Yamaha Grand Piano",
      "size": 7806926
    },
    "1": {
      "file": "001_bright_acoustic_piano.sf2",
      "name": "Bright Yamaha Grand",
      "size": 7806898
    },
    "128": { "file": "128_standard_drums.sf2", "name": "...", "size": 11117498 }
  },
  "fallback": "000_acoustic_grand_piano.sf2"
}
```

### 5.5 sw.js — Service Worker (PWA)

```javascript
const CACHE_NAME = `staveloom-player-${CACHE_VERSION}`; // 현재 CACHE_VERSION = "v1"

// 사전 캐시 (16개)
const STATIC_ASSETS = [
  "/",
  "/index.html",
  "/style.css",
  "/player.js",
  "/virtual-score.js",
  "/soundfont-loader.js",
  "/midi-player.js",
  "/manifest.json",
  "/icon-192.png",
  "/icon-512.png",
  "/pkg/staveloom_wasm.js",
  "/pkg/staveloom_wasm_bg.wasm",
  "/lib/spessasynth_lib.min.js",
  "/lib/spessasynth_processor.min.js",
  "/soundfonts/index.json",
  "/fonts/Bravura-subset.woff2",
];

// install: Promise.allSettled (부분 실패 허용) → skipWaiting()
// activate: 구 캐시 삭제 → clients.claim()
// fetch: cache.match({ ignoreSearch: true }) → 캐시 히트 즉시 반환
//        캐시 미스 → fetch → cache.put → 반환
//        오프라인 navigate → /index.html 폴백
```

**SF2 동적 캐싱**: SF2 파일(악기별 1~5 MB)은 사전 캐시하지 않음. `fetch` 핸들러가 최초 요청 시 자동으로 캐시에 저장 → 이후 오프라인 재생 가능.

---

## 6. 데이터 플로우

### 6.1 웹 플레이어 데이터 플로우

```
[사용자] 파일 드롭 / 선택
       │
       ▼
[player.js] handleFile()
  ├── midiPlayer.unlockAudio()     ← iOS: 제스처 내 AudioContext 생성
  └── FileReader.readAsArrayBuffer()
                │
                ▼ (onload 콜백)
        processFile(name, bytes, fullReset=true)
          │
          ├── isMidiFile(bytes) 로 MIDI(SMF)/MusicXML 분기
          │
          ├── [MIDI 파일인 경우]
          │     ├── [WASM] list_midi_parts(bytes) → PartInfo[]
          │     └── [WASM] parse_midi_and_render(bytes, ...) → RenderResult
          │           └── [Rust] MidiParser::parse() → Score  (midi_parser 모듈, §2.6)
          │
          ├── [MusicXML/.mxl 인 경우]
          │     ├── [WASM] list_parts(bytes) → PartInfo[]
          │     ├── [WASM] list_instruments(bytes) → InstrumentInfo[]
          │     └── [WASM] parse_and_render(bytes, ...) → RenderResult
          │           └── [Rust] parse_in_memory() → Score
          │
          ├── (두 경로 공통, WASM 내부에서 이어짐)
          │     ├── [Rust] Renderer::render_with_metadata() → (SVG, PlayMetadata)
          │     │     ├── analyze_measures() → 마디 너비 배열
          │     │     ├── system_break 계산
          │     │     └── draw_* 메서드 → SVG + BeatEvent 수집
          │     ├── [Rust] SVG → per-system SVGs (extract_system_group)
          │     ├── [Rust] TimelineSolver::solve() → Vec<usize>
          │     └── [Rust] MidiEngine::generate_smf() → SMF bytes
          │
          ├── [virtual-score.js] load(systems)
          │     ├── systems ≤ 10 → _combineSystems() → 단일 SVG DOM 마운트
          │     └── systems > 10 → IntersectionObserver 가상화
          │
          └── [midi-player.js] loadMidi(midi, instruments)  ← 비동기, UI 블로킹 없음
                ├── _ensureAudio() → AudioWorklet + WorkletSynthesizer
                ├── _ensureSoundFonts() → SF2 fetch → mergeSF2Buffers() → addSoundBank()
                └── seq.loadNewSongList() → await 'songChange'

[사용자] Play 버튼 클릭
  └── startPlayback()
        ├── midiPlayer.unlockAudio()    ← iOS: resume()
        ├── await midiPlayer.play()     ← await ctx.resume() → seq.play()
        └── requestAnimationFrame loop
              ├── updateTimelineScrubber()
              ├── updateCursorPosition()  ← BeatEvent 이진 탐색 + 선형 보간
              └── autoScroll()           ← 시스템 경계 스크롤
```

### 6.2 CLI 데이터 플로우

```
$ staveloom score.musicxml --output score.svg --midi out.mid --audio out.mp3 --sf2 font.sf2
                │
                ▼
[staveloom-cli] main()
  ├── load_and_parse(path)             ← .mxl: zip 해제 후 파싱
  │     └── [staveloom-core] parser.parse_in_memory()
  │
  ├── score.filter_parts([P1, P2])    ← --parts 지정 시
  │
  ├── Renderer::render_with_metadata()
  │     └── SVG 문자열 → score.svg 파일 저장
  │
  ├── TimelineSolver::solve()          → Vec<usize>
  │
  ├── MidiEngine::generate_smf()       → out.mid 저장
  │
  └── audio_render::render_mp3()
        ├── rustysynth: SF2 로드 + MidiFileSequencer → PCM f32[]
        └── lame::Lame: PCM → MP3 → out.mp3 저장
```

---

## 7. 핵심 알고리즘

### Elastic Layout 간격 계산

```rust
// 비선형 시간-공간 매핑
W(duration) = C × duration^0.6

// 마디 내 각 onset 위치 계산
x(onset) = margin_left + Σ(i=0..onset) W(duration_i)
```

공식의 의도: 선형 매핑(^1.0)은 빠른 음표가 너무 붙어 보임. 제곱(^0.6)으로 완화하여 음악적 관례에 가까운 간격 실현.

### 우선순위 기반 수직 스태킹 충돌 방지

다이나믹스/가사/코드 표기 등 보표 위아래에 붙는 요소들은 그리는 순서 그대로 두면 서로
겹칩니다. 렌더러는 이들을 즉시 그리지 않고 `StackedElement { priority, time_pos, item }`
목록에 모아뒀다가, 마디를 다 훑은 뒤 `priority`로 정렬해 낮은 숫자(중요도 높음)부터 순서대로
배치합니다 (`renderer/mod.rs`의 `get_priority()` / `PRIORITY STACKING PASS`):

```rust
// 마디 내 시간 위치(time_pos)별 현재까지 점유된 수직 범위
let mut measure_occupancy: HashMap<i32, (f32 /* min_y */, f32 /* max_y */)> = HashMap::new();

// priority 오름차순으로 하나씩 배치하면서, 그 time_pos의 기존 occupancy 아래(혹은 위)에
// 이어 붙이고 occupancy를 갱신 → 다음(낮은 우선순위) 요소가 그 위에 쌓이지 않도록 함
```

우선순위(`get_priority()` + Harmony/Lyric 하드코딩 값): Dynamics(1) > Octave Shift(2) >
Bracket/Dashes(3) > Wedge(4) > Pedal(5) > Segno/Coda/Damp(6) > Words/Other(7) > Harmony(8) >
Lyrics(9) > Metronome(10) > Rehearsal(11)

### SF2 병합 알고리즘

SF2는 RIFF 컨테이너 포맷. 여러 SF2를 병합하려면 각 pdta 섹션의 인덱스를 재계산해야 함:

```
File 1의 기여분 + File 2의 기여분 = 병합 결과

phdr.wPresetBagNdx += cumulative_pbag_count
pbag.wGenNdx       += cumulative_pgen_count
pgen[OpInstrument] += cumulative_inst_count
inst.wInstBagNdx   += cumulative_ibag_count
ibag.wInstGenNdx   += cumulative_igen_count
igen[OpSampleID]   += cumulative_shdr_count
shdr.dwStart/End   += smpl_byte_offset / 2  (sample 단위)
```

### 커서 선형 보간

```javascript
// beatA(시간 ta, 좌표 xa) → beatB(시간 tb, 좌표 xb)
const ratio = (currentTime - ta) / (tb - ta)  // 0.0 ~ 1.0

if (sameLine && xb > xa):
    cursorX = xa + ratio * (xb - xa)  // 부드러운 이동
else:
    cursorX = ratio < 0.85 ? xa : xb  // 시스템 경계: 스냅
```

---

## 8. 배포 및 빌드

### WASM 빌드

```bash
# wasm-build.sh 요약
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

wasm-pack build crates/staveloom-wasm \
  --target web \
  --out-dir ../../web/pkg \
  --release
```

빌드 결과물 (`web/pkg/`):

- `staveloom_wasm_bg.wasm` (1.1 MB) — Rust 컴파일 결과
- `staveloom_wasm.js` (9.9 KB) — JS glue 코드 (wasm-bindgen 생성)

### 개발 서버 (server.py)

Python 표준 라이브러리 `http.server` 기반. 추가 설정:

```python
mimetypes.add_type('application/wasm', '.wasm')          # WASM 스트리밍 인스턴스화
mimetypes.add_type('application/manifest+json', '.webmanifest')

def translate_path(self, path):
    # /              → web/index.html
    # 그 외          → web/<path>  (soundfonts/도 web/ 밑에 있어 별도 라우팅 불필요)

def end_headers(self):
    if self.path.endswith('sw.js'):
        self.send_header('Service-Worker-Allowed', '/')
    super().end_headers()

# HTTP 200/304는 콘솔 출력 억제 (노이즈 최소화)
```

### Bravura 폰트 Subset 생성

```bash
bash scripts/generate_bravura_subset.sh [/path/to/Bravura.otf]
# → pyftsubset으로 212 codepoint subset 추출
# → web/fonts/Bravura-subset.woff2 (28.3 KB) 생성
```

---

## 9. 의존성 트리

### Rust (staveloom-core)

| 크레이트               | 역할                             |
| ---------------------- | -------------------------------- |
| `roxmltree`            | XML DOM 파서 (읽기 전용 트리)    |
| `serde` + `serde_json` | 직렬화/역직렬화                  |
| `svg`                  | SVG 문서 빌더                    |
| `midly`                | SMF MIDI 파일 파서/생성기        |
| `zip`                  | .mxl 압축 해제                   |
| `thiserror`            | 에러 타입                        |
| `walkdir`              | 디렉터리 순회 (테스트/샘플 스캔) |

### Rust (staveloom-cli)

| 크레이트         | 역할                                            |
| ---------------- | ----------------------------------------------- |
| `rustysynth`     | SF2 기반 PCM 합성 (`--audio`/`--midi` 렌더링)   |
| `lame`           | LAME MP3 인코더 바인딩 (`libmp3lame` 동적 링크) |
| `tempfile`       | 오디오 렌더링 중 임시 파일                      |
| `serde_json`     | `--json`/`--metadata` 출력                      |
| `midly`          | `audio_render.rs`에서 MIDI 재생 (SMF 읽기)      |
| `staveloom-core` | 파서, 렌더러, MIDI 엔진                         |

### Rust (staveloom-wasm)

| 크레이트               | 역할                         |
| ---------------------- | ---------------------------- |
| `wasm-bindgen`         | Rust ↔ JS FFI                |
| `serde` + `serde_json` | 결과 구조체 직렬화 도출용    |
| `serde-wasm-bindgen`   | Rust 구조체 → JS 객체 직렬화 |
| `staveloom-core`       | 파서, 렌더러, MIDI 엔진      |

### JavaScript (web/)

| 라이브러리           | 출처                  | 역할                                 |
| -------------------- | --------------------- | ------------------------------------ |
| SpessaSynth          | `lib/*.min.js` (번들) | AudioWorklet SF2 합성기              |
| Web Audio API        | 브라우저 내장         | AudioContext, GainNode, AudioWorklet |
| IntersectionObserver | 브라우저 내장         | 가상 SVG 렌더링                      |
| Service Worker       | 브라우저 내장         | PWA 오프라인 캐시                    |

### 외부 에셋

| 에셋                           | 크기      | 출처                                |
| ------------------------------ | --------- | ----------------------------------- |
| `staveloom_wasm_bg.wasm`       | 1.1 MB    | wasm-pack 빌드                      |
| `spessasynth_lib.min.js`       | 259 KB    | SpessaSynth 릴리즈                  |
| `spessasynth_processor.min.js` | 387 KB    | SpessaSynth 릴리즈                  |
| `Bravura-subset.woff2`         | 28.3 KB   | pyftsubset (전체 500.9 KB에서 추출) |
| `soundfonts/instruments/*.sf2` | 1~5 MB/개 | FluidR3 GM 분할                     |
