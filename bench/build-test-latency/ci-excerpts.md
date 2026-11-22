# Selected baseline log evidence

Retrieve the full log with `gh run view 44987645207 --log`.

```text
UI	UNKNOWN STEP	2026-11-22T05:02:33.3451129Z Version: 20261111.635
UI	UNKNOWN STEP	2026-11-22T05:02:33.3456673Z Image: ubuntu-26.04
UI	UNKNOWN STEP	2026-11-22T05:02:33.3457221Z Version: 20261116.202.1
UI	UNKNOWN STEP	2026-11-22T05:02:42.8547251Z Cache restored successfully
UI	UNKNOWN STEP	2026-11-22T05:04:19.7442275Z    Duration  84.67s (tests 39%, import 31%, environment 21%, transform 5%, setup 4%)
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:33.9468050Z Version: 20261111.635
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:33.9474150Z Image: macos-26-arm64
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:33.9474510Z Version: 20261117.0516.1
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:43.2932620Z   stable-aarch64-apple-darwin unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:43.4789960Z   stable-aarch64-apple-darwin unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:44.0099740Z rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:03:24.5017920Z Cache restored successfully
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:04:56.7572680Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 27s
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:07:29.1070940Z     Finished `test` profile [unoptimized + debuginfo] target(s) in 2m 28s
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:07:29.3606790Z     Starting 2589 tests across 13 binaries
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:09:43.7897390Z         SLOW [> 10.000s] (─────────) ariadne-daemon::it hosts::an_update_names_the_runtime_that_homebrew_did_not_provide
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:09:52.3397150Z         SLOW [> 10.000s] (─────────) ariadne-daemon::it hosts::an_update_upgrades_a_runtime_that_setup_installed_and_leaves_one_the_user_installed
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:09:55.8898480Z         SLOW [> 10.000s] (─────────) ariadne-daemon::it hosts::daemon_version_installs_the_recorded_release_of_a_release_daemon
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:12:47.4090070Z      Summary [ 318.028s] 2589 tests run: 2589 passed (3 slow), 0 skipped
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:12:51.2787620Z     Finished `test` profile [unoptimized + debuginfo] target(s) in 3.01s
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:00.4320170Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 02s
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5834470Z Compile requests                      60
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5835330Z Compile requests executed             18
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5836060Z Cache hits                             0
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5836670Z Cache misses                          18
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5837300Z Cache misses (Rust)                   18
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5837980Z Cache hits rate                     0.00 %
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5838610Z Cache hits rate (Rust)              0.00 %
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5845690Z Non-cacheable calls                   42
Rust (macos-latest)	UNKNOWN STEP	2026-11-22T05:14:19.5851700Z crate-type                            35
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:32.7134580Z Version: 20261111.635
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:32.7178640Z Image: macos-26-arm64
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:32.7179040Z Version: 20261117.0516.1
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:37.6408840Z   stable-aarch64-apple-darwin unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:37.7385570Z   stable-aarch64-apple-darwin unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:38.1106730Z rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:43.0610860Z Cache restored successfully
Tauri shell (macos-latest)	UNKNOWN STEP	2026-11-22T05:02:49.6455160Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.50s
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:34.0613609Z Version: 20261111.635
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:34.0623156Z Image: ubuntu-26.04
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:34.0623741Z Version: 20261116.202.1
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:38.2726129Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:38.3050341Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rustfmt	UNKNOWN STEP	2026-11-22T05:02:38.4874876Z rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:33.2125862Z Version: 20261111.635
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:33.2131343Z Image: ubuntu-26.04
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:33.2131698Z Version: 20261116.202.1
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:41.2814071Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:41.3672317Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:41.5654208Z rustc 1.100.0 (0ef1bc386 2026-11-09)
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:03:00.3433039Z Cache restored successfully
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:03:48.7672702Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 45.21s
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:05:00.8501464Z     Finished `test` profile [unoptimized + debuginfo] target(s) in 1m 11s
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:05:00.9903534Z     Starting 2587 tests across 13 binaries
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:07:21.7308121Z      Summary [ 140.741s] 2587 tests run: 2587 passed (1 flaky), 0 skipped
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:07:21.9482908Z     Finished `test` profile [unoptimized + debuginfo] target(s) in 0.17s
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:07:55.8582236Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 31.47s
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4882915Z Compile requests                     60
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4883208Z Compile requests executed            12
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4883502Z Cache hits                            0
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4883741Z Cache misses                         12
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4883901Z Cache misses (Rust)                  12
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4884058Z Cache hits rate                    0.00 %
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4884228Z Cache hits rate (Rust)             0.00 %
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4886047Z Non-cacheable calls                  48
Rust (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:08:09.4888283Z crate-type                           18
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:34.2422873Z Version: 20261111.635
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:34.2429755Z Image: ubuntu-26.04
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:34.2430191Z Version: 20261116.88.1
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:46.8906340Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:46.9104218Z   stable-x86_64-unknown-linux-gnu unchanged - rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:47.0313447Z rustc 1.100.0 (0ef1bc386 2026-11-09)
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:02:55.1185497Z Cache restored successfully
Tauri shell (ubuntu-latest)	UNKNOWN STEP	2026-11-22T05:03:50.8459535Z     Finished `dev` profile [unoptimized + debuginfo] target(s) in 10.61s
Dashboard	UNKNOWN STEP	2026-11-22T05:02:32.8224381Z Version: 20261111.635
Dashboard	UNKNOWN STEP	2026-11-22T05:02:32.8230316Z Image: ubuntu-26.04
Dashboard	UNKNOWN STEP	2026-11-22T05:02:32.8230682Z Version: 20261116.202.1
Dashboard	UNKNOWN STEP	2026-11-22T05:02:39.8065962Z Cache restored successfully
Dashboard	UNKNOWN STEP	2026-11-22T05:02:51.9745539Z    Duration  2.47s (import 51%, environment 25%, setup 13%, transform 6%, tests 3%, worker 1%)
```
