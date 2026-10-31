# TimeSchedule 0.7.0 실행 파일 정리 검증

2026-10-31, `codex/unified-native-release`. 승인받은 PR #2–#6을 합친 main `d604ad7` 위에서 진행했다. 새 버전·빌드 정리는 별도 PR로 검토한다.

## 현재 실행 파일

- `C:\LSH\TimeSchedule\artifacts\TimeSchedule.exe`
- Rust/Iced 0.7.0, 5,828,608 bytes.
- SHA-256: `ABAF90F3DA939715B2E24C3F786AC620B12989CDBBF85A23AB488094744E443C`.
- Cargo 패키지, Windows FileVersion/ProductVersion, 창 제목, 설정의 버전 표시가 같은 기준을 사용한다.
- 최상위 artifacts의 EXE는 하나다. 이전 이름별 EXE 14개는 `artifacts/archive/legacy-20261031/`에 옮기고 이동 전후 SHA-256을 대조했다. 해당 폴더의 `inventory.json`에 원래 이름·경로·크기·해시를 기록했다.
- 기존 `artifacts/TimeSchedule.exe`도 빌드의 원자적 교체로 `artifacts/archive/TimeSchedule-20261031-161440-8d81691c27224653a47dde1d6ffa3c97.exe`에 보관했다. 기존 EXE 총 15개를 보존했다.

## 확인한 결과

- `cargo test --workspace --locked --offline`: **95개 통과, 의도된 제외 2개**.
- 병합 직전 별도 Windows 창 fixture 1개, 기존 Tauri 테스트, Node 3개, Svelte 검사 0 오류/경고 통과. 병합 단계마다 결과 파일 트리가 테스트한 `f00799c`와 동일한지 확인했다.
- `scripts/build-native.ps1 -Offline`: 네이티브 테스트 56개 통과/1개 제외 및 릴리스 빌드 성공. 형식 검사와 `git diff --check` 통과.
- `scripts/tests/publish-native.tests.ps1 -CheckRunningApp`: 현재 버전 리소스, 첫 배포, 정확한 바이트, 이전 EXE 보관, 버전 불일치·없는 원본·독점 잠금 거부, 임시 파일 정리, 실제 실행 중 앱 교체 거부와 프로세스 보존 확인.
- 실행 중인 이미지도 Windows의 원자적 교체가 허용하는 경우를 실제로 확인했다. 따라서 파일 잠금만 믿지 않고 해당 출력 경로를 실행하는 프로세스도 검사한다. 이 회귀 검사는 실패 후 통과했다.
- 독립 검토가 발견한 과거 벤치마크 기본 경로 문제를 수정했다. `scripts/tests/benchmark-archive.tests.ps1`로 보관된 네이티브 원본과 기존 Tauri 기준판의 해시를 검사하는 경로까지 실행했다. 누락된 테스트 fixture에서 중단하므로 앱은 실행하지 않는다. 새 성능 수치를 측정한 것은 아니다.
- 독립 데이터로 실행해 창 제목 `TimeSchedule 0.7.0 · 만년 다이어리`, SQLite 열기, 두 창 준비와 프로세스 응답을 확인했다. 제한된 실행 환경에서는 Explorer 연결이 실패했고, 일반 Windows 세션에서 다시 실행해 `attached=true`, `visible=true`, 시작 오류 없음을 확인했다.

사용자 앱은 저장·종료 안내 후 사용자가 종료했다. 실행 검증은 별도 데이터 폴더와 테스트 소유 프로세스만 사용했으며 그 프로세스만 정리했다. 기존 사용자 데이터와 기본 저장 폴더는 변경하지 않았다. 버전 표시는 실제 창 제목과 파일 속성으로 확인했으며, 설정 화면의 실제 클릭·한글 IME·DPI/다중 모니터 조작은 이번에 다시 검증하지 않았다.

## 사용자 확인

1. `artifacts/TimeSchedule.exe`를 실행한다. 앞으로 이 경로만 사용한다.
2. 창 제목·설정 및 EXE의 속성 → 자세히에서 `0.7.0`을 확인한다.
3. 기존 네이티브 데이터와 위젯 설정이 남아 있는지, 일정 저장·년/월 이동이 정상인지 확인한다.
4. 확인 후 새 정리 PR의 머지를 승인한다. 이미 승인한 기능 PR #2–#6은 병합 완료 상태다.

기본 데이터 경로는 `%LOCALAPPDATA%\com.timeschedule.personal.native-experiment`로 유지한다. 기존 Tauri 데이터의 자동 이전은 하지 않는다.
