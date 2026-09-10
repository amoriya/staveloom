*[English version](README.md)*

# Staveloom

**Staveloom**은 Rust로 작성된 MusicXML 파싱 및 SVG 렌더링 엔진입니다. CLI 도구와 브라우저 기반 대화형 웹 플레이어를 함께 제공합니다.

> **웹 플레이어는 완전히 클라이언트 사이드로 동작합니다.** 렌더링(WASM), MIDI 합성(SpessaSynth), 재생 모두 서버 없이 브라우저 안에서 이루어집니다.

**[라이브 데모 바로가기 →](https://amoriya.github.io/staveloom/)**

![Staveloom 웹 플레이어](docs/demo.gif)

---

## 주요 기능

- **MusicXML 3.1/4.0 & `.mxl`** 파싱 — 다양한 표기 소프트웨어가 내보낸 비표준 구조도 견고하게 처리
- **3가지 레이아웃 모드**: Compact(기본), Elastic(음표 길이 기반 간격), Mobile(좁은 화면 자동 감지, 조밀 배치)
- **풍부한 표기법 지원**: 빔/슬러/타이, 기타 타브 및 벤드, 잇단음표, 장식음, 다이나믹스/가사/코드 겹침 없는 배치
- **MIDI/MP3 내보내기** (CLI): 트릴·모르덴트·아르페지오·트레몰로를 실제 음표 시퀀스로 전개
- **재생 동기화**: 도돌이표, 볼타, D.S./D.C./Segno/Coda를 해석해 커서 추적용 박자 단위 타이밍 생성
- **대화형 웹 플레이어**: MusicXML/MIDI 드래그 앤 드롭, 실시간 SF2 합성, 대형 악보용 가상 렌더링, 오프라인 지원 PWA

## 빠른 시작

### CLI

```bash
cargo build --release
cargo run -- score.musicxml --output score.svg
cargo run -- score.musicxml --midi out.mid --sf2 font.sf2 --audio out.mp3
```

인수 없이 실행하면 전체 옵션(파트 필터링, 레이아웃 모드, 페이지 너비 등)을 확인할 수 있습니다.

### 웹 플레이어

```bash
bash wasm-build.sh   # WASM 최초 빌드 또는 Rust 코드 변경 시
python3 server.py    # 개발 서버 시작
```

`http://localhost:8000`을 열고 `.xml`, `.musicxml`, `.mxl`, `.mid` 파일을 드래그 앤 드롭하면 됩니다.

## 기술 스택

Rust (roxmltree, svg, midly, rustysynth, lame) · WASM (wasm-bindgen) · 웹 플레이어는 순수 JS + SpessaSynth · SMuFL/Bravura 폰트

## 테스트

```bash
cargo test
```

`scripts/generate_report.py`, `scripts/generate_preview.py`는 전체 테스트 샘플을 브라우저에서 볼 수 있는 HTML(`specification_report.html`, `preview.html`)로 렌더링해 시각적 회귀 검증에 사용합니다.

## 라이선스

**MIT OR Apache-2.0** 듀얼 라이선스 — 원하는 쪽을 선택하면 됩니다. 전문은
[`LICENSE-MIT`](LICENSE-MIT) / [`LICENSE-APACHE`](LICENSE-APACHE)를 참고하세요.

번들된 서드파티 구성 요소(SpessaSynth, Bravura, FluidR3 GM 등)에 대한 검토는
[`docs/THIRDPARTY_LICENSE.md`](docs/THIRDPARTY_LICENSE.md)에 있습니다.
