# 자리배치

학생들의 자리를 정하는 교사용 프로그램입니다.

교사가 평소 말투로 적은 조건("철수랑 영희 멀리", "민수 시력 안 좋음")을 프로그램에 내장된 소형 LLM이 규칙으로 바꾸고, 알고리즘이 그 규칙을 지키는 무작위 배치를 만듭니다. 결과는 전자칠판에서 한 명씩 공개합니다. 설치, 계정, 서버, API 키가 필요 없습니다.

## 내려받기
**[자리배치 내려받기 (Windows, zip)](https://github.com/minjun232/class-pairing/releases/latest/download/jaribaechi-windows.zip)**

압축을 풀고 `자리배치` 폴더 안의 `자리배치.exe`를 실행하세요. 설치는 필요 없습니다. 이전 버전은 [Releases](https://github.com/minjun232/class-pairing/releases)에 있습니다.

## 문서
- [사용설명서 (PDF)](https://github.com/minjun232/class-pairing/releases/latest/download/jaribaechi-manual.pdf)

## 구조
| 경로 | 내용 |
|---|---|
| `crates/seat-core` | 교실 모양, 규칙(자리 집합·두 사람 거리), 배치 알고리즘 |
| `crates/seat-interpret` | 이름 인식, 프롬프트, 출력 문법(GBNF), 결과 검증 |
| `crates/seat-llm` | llama.cpp로 GGUF 모델 실행 |
| `crates/seat-storage` | 명단.xlsx, Settings.ini, 상태 파일, Word 출력 |
| `app/` | Tauri 2 앱 (Windows 단일 exe) |

## 라이선스
[MIT License](LICENSE) — 누구나 무료로 쓰고, 고치고, 다시 배포할 수 있습니다. 함께 배포하는 모델(Qwen2.5, Apache 2.0)과 라이브러리의 라이선스는 배포 파일의 `system/LICENSE`에 있습니다.

---

Copyright © aa_sae__
