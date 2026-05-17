# 测试 headless 化方案

> 状态：✅ 已实施（PR #4，2026-05-16）· 实际偏离与坑见文末「实施记录」 · 作者：Claude Opus 4.7 协助

## 背景

`examples/` 下所有 VDP 集成测试目前共 24 处 `#[ignore]`，全部因为引擎强依赖窗口系统：

- `crates/vibe_platform/src/desktop.rs:32-43` 直接 `Window::default_attributes()` 创建可见窗口，没有任何隐藏 / 关闭开关
- `crates/vibe_test/src/lib.rs:323-335` 直接 `cargo run -p <pkg>`，子进程必然弹窗
- `.github/workflows/ci.yml:3-8` 明确声明「VDP 集成测试需要 GPU/display server，CI runner 提供不了，所以本地跑」

后果：
1. **本地痛点**：开发者跑 `cargo test -- --ignored` 时一堆窗口弹出来抢焦点
2. **CI 痛点**：12 个 tactics-demo VDP 测试 + 其他 demo 总计 24 个 ignored 测试，**全部不在 CI 跑**。已经依赖作者本地运行，存在「跑通本地 → 推 → CI 全绿但实际场景没验」的盲区
3. **历史证据**：PR #2 的 Copilot 评论显示它在自己的环境里用了 **Xvfb + lavapipe** 截到了真实游戏画面 → 其实可以做到，只是当前 repo 没沉淀这套配置

## 目标

1. 本地（macOS）跑 `--ignored` 不再弹窗
2. Linux CI 能跑 24 个 ignored VDP 集成测试，每次 PR 都过这层 gate
3. 不破坏现有「人工 debug 看真窗口」的能力
4. 改动尽可能小、可逆，避免引擎大重构

## 非目标

- 不做 wgpu 真正的 surfaceless / offscreen 重构（C 方案，作为未来改进登记）
- 不引入新的渲染后端、不改 wgpu 版本
- 不强制 macOS CI 跑 VDP 测试（XQuartz 太折腾，价值有限）
- 不动现有任何已通过的 unit test

## 三个候选方案

### A：隐藏窗口模式（macOS 本地痛点）

**做什么**：让游戏进程支持「窗口仍然创建，但 winit 设为不可见」。

**改动文件**

1. `crates/vibe_platform/src/desktop.rs`（约 +6 行）

   ```rust
   // 在 resumed() 里读环境变量
   let mut win_attrs = Window::default_attributes()
       .with_title(&self.config.window_title)
       .with_inner_size(winit::dpi::LogicalSize::new(
           self.config.window_width,
           self.config.window_height,
       ));
   if std::env::var("VIBE_HIDDEN_WINDOW").is_ok() {
       win_attrs = win_attrs.with_visible(false);
   }
   ```

2. `crates/vibe_test/src/lib.rs`（约 +15 行）

   `LaunchOptions` 增加 `visible: bool`（默认 `false`）。`launch_with` 在 `visible == false` 时给子进程设 `VIBE_HIDDEN_WINDOW=1`。提供 `LaunchOptions::visible(true)` 用于人工 debug 看真窗口。

**取舍**

- 优点：5 分钟写完，立即解决你本地弹窗问题；window 对象仍存在 → wgpu surface / screenshot 都不变
- 缺点：仍然依赖 display server（macOS Quartz / Linux X11/Wayland）。**不能让 Linux CI 跑起来**——GitHub runner 默认无 display
- 适用范围：mac / Windows / 已有 display 的 Linux 工作站

**估时**：30 分钟（含改动 + 验证 + commit）

---

### B：Xvfb + lavapipe CI 工作流（Linux CI 真跑）

**做什么**：在 GitHub Actions ubuntu-latest 上装虚拟 display + 软件 Vulkan，让 `--ignored` 测试在 CI 里真跑起来。这就是 PR #2 Copilot 用过的同一套。

**改动文件**

1. `.github/workflows/ci.yml`（约 +30 行）— 新增 job `vdp-integration`：

   ```yaml
   vdp-integration:
     name: VDP integration tests (headless)
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4
       - uses: dtolnay/rust-toolchain@stable
       - uses: Swatinem/rust-cache@v2
       - name: Install audio + display + Vulkan deps
         run: |
           sudo apt-get update
           sudo apt-get install -y \
             libasound2-dev \
             xvfb \
             mesa-vulkan-drivers \
             vulkan-tools
       - name: Verify lavapipe is available
         run: xvfb-run -a vulkaninfo --summary | grep -i llvmpipe
       - name: Run ignored VDP integration tests under Xvfb
         env:
           WGPU_BACKEND: vulkan
           RUST_LOG: warn
         run: |
           xvfb-run -a cargo test --workspace --release \
             -- --ignored --test-threads=1
   ```

   要点：
   - `xvfb-run -a` 自动选空闲 DISPLAY 编号，跑完关闭虚拟屏
   - `WGPU_BACKEND=vulkan` 强制 wgpu 走 Vulkan（lavapipe），跳过 GL/Metal 探测
   - `--release` 让 demo 跑得动一些，软件 Vulkan 慢；`--test-threads=1` 避免端口冲突（多个游戏同时占 9229/9233）
   - 跑前 `vulkaninfo` 一行 verify，跑挂可以快速定位是 lavapipe 装错而不是测试逻辑挂

2. `.github/workflows/ci.yml:3-8` 顶部注释更新：把「CI 不跑 VDP 测试」改成「主 test job 跳过；vdp-integration job 用 Xvfb 跑」

3. `crates/vibe_test/src/lib.rs`（约 +10 行）— **harness 透传 `--release` 给子进程**

   `cargo test --release` 只让测试二进制是 release，但 `GameHarness::launch_with` 的 `cargo run --quiet -p <pkg>` 仍然起 debug 版游戏子进程。lavapipe 跑 debug 太慢（容易测试超时）。需要 `LaunchOptions::release(bool)` 字段，开启时 cmd args 改为 `["run", "--release", "--quiet", "-p", opts.package]`。CI 测试里显式 `LaunchOptions::new(...).release(true)`，本地仍可保持 debug 加速迭代。

**取舍**

- 优点：真正把 24 个 ignored 测试纳入 CI 门禁。每个 PR 都验过 VDP / 渲染路径
- 缺点：
  - 软件 Vulkan 慢，估计每个 game 启动 + 跑测试 ≈ 30-60s，全部串行可能 5-10 分钟
  - lavapipe 不是所有 wgpu feature 都支持。第一次跑可能撞到具体问题，要逐个排查
  - 只覆盖 Linux；mac/Windows runner 上还是没法跑（mac 上的方案见 A）
  - 现有 12 个 tactics-demo VDP 测试当前都用 `LaunchOptions::new(...)`（debug），本期需要批量改成 `release(true)` 或在 test default 里改默认。建议加默认开关 `VIBE_TEST_RELEASE=1` 由 CI workflow 注入，本地不变
- 适用范围：Linux CI

**估时**：1-2 小时（CI 跑通 + 调 lavapipe 兼容性问题 + harness 改动）

---

### D：PR 评审触发的演示 GIF（CI 录制 + 评论回贴）

**做什么**：PR 评审者想看实际玩法时，触发一个 CI job：在 Xvfb 上录一段 demo 真跑、转 GIF、自动作为 PR 评论贴出来。区别于 B 方案的回归测试 job，这个 job 是「演示用」，只在评审时跑，不阻塞合入。

#### 触发方式：纯路径过滤自动跑

视觉相关路径变更时自动录，评审者啥都不用做；纯文档 / CI / refactor 改动不录省 CI 分钟。**不引入 label 机制。**

```yaml
on:
  pull_request:
    paths:
      - 'crates/vibe_render/**'
      - 'crates/vibe_ui/**'
      - 'crates/vibe_platform/**'
      - 'examples/**'
```

行为：PR 改了 `paths` 列表里任一路径 → 自动录；没改 → 跳过。零评审者操作。

#### GIF 托管 + 贴评论（关键决策）

GitHub 评论不支持二进制附件，需要先把 GIF 放到一个能稳定外链的位置。三个方案：

| 托管 | 评论里能否 inline 渲染 | 操作复杂度 |
|---|---|---|
| `actions/upload-artifact` | ❌ artifact URL 要登录才能下，无法 `<img src>` | 最简单 |
| 推到仓库的 `assets` 分支，引用 `raw.githubusercontent.com/...` URL | ✅ inline 显示 | 中等，需要写 push 步骤 |
| 推到 `gh-pages` 分支并启用 Pages | ✅ inline 显示 | 中等，要先启用 Pages |

**推荐**：仓库新建独立 orphan 分支 `playthrough-assets`（不污染 main 历史），workflow push GIF 到 `playthrough-assets/pr-NNN-<sha>.gif`，评论里写

```markdown
![tactics-demo playthrough](https://raw.githubusercontent.com/<owner>/<repo>/playthrough-assets/pr-42-abc1234.gif)
```

raw URL inline 渲染、无需 Pages、清理简单（workflow 在 PR closed/merged 时再删一遍）。

#### 安全模型：双 workflow 隔离（重要）

**问题**：单 job 里同时 `actions/checkout` PR 代码 + `cargo test` 跑 PR 控制的代码 + 拥有 `contents: write` / `pull-requests: write` token，意味着 PR 作者可以在 `cargo test` 时改测试代码读 / 用这个 token，构成 supply chain 攻击面。Codex 评审专门点了这条。

**解法**：拆成 GitHub 官方推荐的 [双 workflow 模式](https://securitylab.github.com/research/github-actions-preventing-pwn-requests/)：

- **Workflow 1：`playthrough-record.yml`** — `pull_request` 触发（路径过滤，见上）
  - 权限 `permissions: read-all`（不签发 write token）
  - checkout PR 代码、跑 `cargo test`、生成 GIF、把 GIF + PR 元信息（pr_number、head_sha）作为 artifact 上传
  - 即使 PR 作者篡改测试代码，最坏情况是产出一个错的 GIF，无法触达任何写权限
- **Workflow 2：`playthrough-publish.yml`** — `workflow_run` 触发（监听 record workflow `completed` 且 `conclusion == 'success'`）
  - 权限 `permissions: contents: write, pull-requests: write`
  - **不 checkout PR 代码**；只用 `actions/download-artifact@v4` 拿 record 产出的 GIF 和元信息
  - push GIF 到 `playthrough-assets` 分支、用 `actions/github-script` 给 PR 评论
  - 整个 job 不执行任何 PR 控制的代码 → write token 安全

**改动文件**

1. `.github/workflows/playthrough-record.yml`（新文件，约 60 行）

   ```yaml
   name: PR Playthrough Record

   on:
     pull_request:
       paths:
         - 'crates/vibe_render/**'
         - 'crates/vibe_ui/**'
         - 'crates/vibe_platform/**'
         - 'examples/**'

   jobs:
     record:
       runs-on: ubuntu-latest
       permissions: read-all   # 关键：不签发 write token
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
         - uses: Swatinem/rust-cache@v2

         - name: Install deps
           run: |
             sudo apt-get update
             sudo apt-get install -y \
               libasound2-dev xvfb mesa-vulkan-drivers \
               ffmpeg gifski

         - name: Start Xvfb + ffmpeg recorder
           run: |
             Xvfb :99 -screen 0 960x640x24 &
             echo "DISPLAY=:99" >> $GITHUB_ENV
             sleep 1
             ffmpeg -y -f x11grab -video_size 960x640 -framerate 30 \
               -i :99 -c:v libx264 -pix_fmt yuv420p \
               /tmp/play.mp4 &
             echo $! > /tmp/ffmpeg.pid

         - name: Run playthrough scenario
           env:
             WGPU_BACKEND: vulkan
             RUST_LOG: warn
             VIBE_TEST_RELEASE: "1"   # 见 B 方案 harness 改动
           run: |
             cargo test -p tactics-demo --release \
               --test playthrough -- --ignored --nocapture

         - name: Stop recorder + convert to GIF
           run: |
             kill -INT $(cat /tmp/ffmpeg.pid)
             sleep 2
             gifski -o /tmp/play.gif --fps 15 \
               --width 720 --quality 85 /tmp/play.mp4

         - name: Save PR metadata for publish job
           run: |
             mkdir -p /tmp/meta
             echo "${{ github.event.pull_request.number }}" > /tmp/meta/pr_number
             echo "${{ github.event.pull_request.head.sha }}" > /tmp/meta/head_sha

         - uses: actions/upload-artifact@v4
           with:
             name: playthrough
             path: |
               /tmp/play.gif
               /tmp/meta/
             retention-days: 7
   ```

2. `.github/workflows/playthrough-publish.yml`（新文件，约 60 行）

   ```yaml
   name: PR Playthrough Publish

   on:
     workflow_run:
       workflows: ["PR Playthrough Record"]
       types: [completed]

   jobs:
     publish:
       if: github.event.workflow_run.conclusion == 'success'
       runs-on: ubuntu-latest
       permissions:
         contents: write
         pull-requests: write
       steps:
         # 关键：不 checkout PR 代码。只 checkout 自己仓库的 playthrough-assets 分支。
         - uses: actions/checkout@v4
           with:
             ref: playthrough-assets
             # 没有 ref 时 workflow_run 默认 checkout 默认分支，这里显式指向 assets 分支

         - name: Download artifact from record workflow
           uses: actions/download-artifact@v4
           with:
             name: playthrough
             run-id: ${{ github.event.workflow_run.id }}
             github-token: ${{ github.token }}
             path: /tmp/dl

         - name: Read PR metadata
           id: meta
           run: |
             echo "pr=$(cat /tmp/dl/meta/pr_number)" >> $GITHUB_OUTPUT
             echo "sha=$(cat /tmp/dl/meta/head_sha)" >> $GITHUB_OUTPUT

         - name: Commit GIF to assets branch
           env:
             PR: ${{ steps.meta.outputs.pr }}
             SHA: ${{ steps.meta.outputs.sha }}
           run: |
             SHORT="${SHA:0:7}"
             cp /tmp/dl/play.gif "pr-${PR}-${SHORT}.gif"
             git config user.name "github-actions[bot]"
             git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
             git add "pr-${PR}-${SHORT}.gif"
             git commit -m "playthrough: pr-${PR} @ ${SHORT}"
             git push

         - uses: actions/github-script@v7
           env:
             PR: ${{ steps.meta.outputs.pr }}
             SHA: ${{ steps.meta.outputs.sha }}
           with:
             script: |
               const pr = parseInt(process.env.PR);
               const short = process.env.SHA.slice(0, 7);
               const url = `https://raw.githubusercontent.com/${context.repo.owner}/${context.repo.repo}/playthrough-assets/pr-${pr}-${short}.gif`;
               github.rest.issues.createComment({
                 owner: context.repo.owner,
                 repo: context.repo.repo,
                 issue_number: pr,
                 body: `### 🎮 Playthrough\n\n![](${url})\n\n*Auto-generated from ${short} via Xvfb + lavapipe + ffmpeg.*`,
               });
   ```

3. `examples/tactics-demo/tests/playthrough.rs`（新文件，约 80-120 行）

   场景脚本，纯 VDP 驱动，加合理 sleep 让动作看着像人玩：

   ```rust
   #[tokio::test(flavor = "multi_thread")]
   #[ignore = "demo recording — used by .github/workflows/playthrough.yml"]
   async fn full_playthrough() {
       let mut h = GameHarness::launch(GAME_PACKAGE, VDP_PORT).await.unwrap();
       // 不 pause —— 录制需要让游戏正常 running 来产生帧
       sleep_human().await;       // 让初始画面亮一会儿

       // Turn 1: 选 Cain → 走到中央 → Wait
       h.call("game.selectUnit", json!({ "id": 2 })).await.unwrap();
       sleep_human().await;
       h.call("game.moveSelected", json!({ "x": 4, "y": 4 })).await.unwrap();
       sleep_human().await;
       h.call("game.waitSelected", json!({})).await.unwrap();

       // ... 其他几个单位，最后 endTurn 看敌方阶段动画
       // ... 几个回合后达成 victory
   }
   ```

   `sleep_human()` 大约 600-1000ms，让人眼能看清每一步。

4. `.github/workflows/playthrough-cleanup.yml`（新文件，约 30 行）

   PR closed 时删除对应 `playthrough-assets/pr-NNN-*.gif`，避免分支无限膨胀。

**取舍**

- 优点：PR 评审者直接在评论里看到「这次改动跑起来真长这样」，对 visual / UX 改动尤其有价值
- 缺点：
  - 跑一次 ≈ 5-8 分钟（Xvfb 启动 + 软件 Vulkan 编译/启动 + 30-60s 录制 + 转 GIF）
  - 演示脚本需要随玩法变更维护（破坏性修改要同步改 playthrough.rs）
  - lavapipe 帧率不高，最终 GIF 看着会有点卡，但能反映真实渲染
  - GIF 文件随 PR 累积在 `playthrough-assets` 分支，需要 cleanup 工作流（已包含）
- 适用范围：仅 Linux CI

**估时**：3-4 小时（playthrough 脚本调时序最花时间，CI workflow + 托管发布约 1 小时）

---

### C：真正 surfaceless / offscreen wgpu（未来改进，本期不做）

**做什么**：让引擎平台层支持「不创建任何窗口、不创建 surface，渲染目标是一张 `wgpu::Texture`」的纯 offscreen 模式。

**改动量**：

- `crates/vibe_platform/src/desktop.rs` 拆成 `windowed` / `offscreen` 两条路径
- 渲染管线 swapchain → texture，screenshot 路径已经差不多是这个了，可以复用
- `Game::update` 还在跑事件循环，但事件循环不依赖 winit window
- 新增 mode 切换：env var 或 `vibe2d::run_offscreen` 入口

**取舍**

- 优点：真正零依赖，CI / Mac / Windows / Linux 哪都能跑，不再需要 Xvfb / XQuartz
- 缺点：是个真重构，几个小时；要谨慎处理输入路径（offscreen 模式输入只能从 VDP 来）
- 适用范围：所有平台

**估时**：4-6 小时

**结论**：登记到 `docs/engine_quirks.md` 作为后续 TODO，本期不做。等 A + B 跑顺了，等到 mac CI 也想跑测试 / 想去掉 Xvfb 依赖时再回来做。

---

## 推荐组合：A + B + D，C 仅记录

按这个顺序：

1. **A 先做** — 解决你本地弹窗，30 分钟见效，独立提交
2. **B 紧跟** — 加 CI 工作流跑 ignored 测试。同时落 harness `release(true)` 透传（P2 评审意见），独立提交
3. **D 再加** — 演示 GIF 录制 + PR 评论。双 workflow 安全模式（P1 评审意见），独立提交
4. **C 仅登记** — 在 `docs/engine_quirks.md` 第 2 条记录「真 offscreen wgpu」TODO，附本规划文档链接

A、B、D 互不依赖，可以分别 review、分别合入。D 依赖 B 的 harness 改动（`VIBE_TEST_RELEASE` 环境变量），所以顺序上 B 先 D 后。

## 提交计划

| 步 | 提交标题 | 改动 |
|---|---|---|
| 1 | `feat(platform): hidden window mode via VIBE_HIDDEN_WINDOW env` | `desktop.rs` +6 行；`vibe_test` `LaunchOptions::visible` +15 行 |
| 2 | `feat(vibe_test): release-mode child via LaunchOptions::release` | `vibe_test/src/lib.rs` +10 行；`VIBE_TEST_RELEASE` env override |
| 3 | `ci: run ignored VDP tests under Xvfb + lavapipe` | `.github/workflows/ci.yml` +30 行新 job |
| 4 | `feat(tactics-demo): playthrough scenario test` | `tests/playthrough.rs` +80-120 行 |
| 5 | `ci: PR playthrough recording (record + publish split)` | 2 个新 workflow + 1 个 cleanup workflow |
| 6 | `docs(engine_quirks): add offscreen wgpu TODO entry` | `engine_quirks.md` +20 行新条目 |

## 验证清单

A 完成后：

- [ ] `cargo test -p tactics-demo -- --ignored --test-threads=1` 在 macOS 不弹窗（或窗口隐藏）
- [ ] 12/12 集成测试仍通过
- [ ] 显式 `LaunchOptions::visible(true)` 仍能看到窗口（人工 debug 路径未坏）

B 完成后：

- [ ] CI 新 job 跑通，24 个 ignored 测试全绿
- [ ] CI 时长可接受（< 15 分钟新增）
- [ ] 主 test job 行为不变（仍只跑 unit + lib + bins）
- [ ] `VIBE_TEST_RELEASE=1` 时 harness 起的子进程是 release 构建（用 `ps`/`/proc` 或日志 verify）

D 完成后：

- [ ] 推一个改 `examples/` 的测试 PR，record workflow 自动触发；推一个只改 `docs/` 的 PR，不触发
- [ ] record workflow 没有 write token 但能产 artifact
- [ ] publish workflow 不 checkout PR 代码、能下载 artifact、push 到 `playthrough-assets` 分支、PR 评论 inline 显示 GIF
- [ ] PR closed 后 cleanup workflow 删掉对应 GIF 文件

## 待你决策的开放问题

**Q1 — 环境变量名**：`VIBE_HIDDEN_WINDOW` vs `VIBE_HEADLESS` vs `VIBE2D_HIDDEN`？

✅ 已决：`VIBE_HEADLESS`

---

**Q2 — CI 试水范围**：B 先只跑 tactics-demo 还是直接 `--workspace`（24 个 ignored 全开）？

✅ 已决：先单独跑 tactics-demo

---

**Q3 — release 默认**：`VIBE_TEST_RELEASE` 默认 off（本地保持 debug 加速）vs 跟随 `cargo test --release` 自动？

✅ 已决：默认 off，本地保持 debug 加速

---

**Q4 — D 触发方式**：~~`record-demo` label / `/record-demo` 评论 slash command / `workflow_dispatch` 手动？~~

✅ 已决：纯路径过滤自动跑，不引入 label

---

**Q5 — D 托管 GIF**：~~`playthrough-assets` 分支 vs `gh-pages` vs main 仓库内 `playthroughs/` 目录？~~

✅ 已决：orphan 分支 `playthrough-assets`。

确认调研：GitHub 公开 API 不支持把二进制附件直接放进 PR 评论（`IssueComment` body 只接 markdown 文本；`<img src="data:...base64,...">` 会被服务端剥；`user-attachments` 端点只对 Web UI drag-drop 暴露）。`Argos` / `Chromatic` 等主流视觉测试工具都走自家 SaaS 托管。仓库内 hosting 唯一干净方案就是 orphan 分支：main 历史干净、fork PR 友好（publish workflow 在 base repo 跑）、cleanup 容易（force-push 重建即可）。

---

**Q6 — D 自动跑**：~~默认所有 PR 都跑录制，还是仅 label 触发？~~

✅ 已决：路径匹配的 PR 自动跑（结合 Q4 的纯路径过滤方案）

---

**Q7 — C 优先级**：是「以后再说」还是「记 TODO 但不一定做」？

✅ 已决：不用做

---

所有 7 个开放问题已决，可以开工。

---

## 实施记录（2026-05-16，PR #4）

### 实际偏离

- **目标 example 改成 `ui-demo`**：规划中的 `tactics-demo` 在当时仓库不存在（仅 aoi-demo / flappy-bird / mari0 / tetris / ui）。CI gate（Step 3）和 playthrough（Step 4）全部绑到 ui-demo。
- **Step 6 跳过**：`docs/engine_quirks.md` 不存在，本期不新建。
- **D 方案从 3 workflow 合成 1 个**：原设计 `playthrough-record.yml`（pull_request 触发，read-only）+ `playthrough-publish.yml`（workflow_run 触发，write 权限）撞上 GitHub 平台硬规则——`workflow_run` 仅承认默认分支上的 workflow 文件。publish workflow 死活注册不上。最终改成单 `.github/workflows/playthrough.yml` + `pull_request_target` + 按 job 隔离 permissions：record 用 `contents: read` 跑 PR 代码；publish/cleanup 用 write 权限但**绝不 checkout PR HEAD**，只摸 `playthrough-assets` 分支。

### CI 调试踩到的真坑（按时间顺序）

1. **`gifski` 不在 apt**：它是 Rust 二进制，apt 装不上。换成 ffmpeg `palettegen → paletteuse` 两遍——质量接近，少一个依赖。
2. **子进程 stdio 被 null 吞掉**：harness 默认 `Stdio::null()` 让 CI 失败时只看到 "VDP-ready timeout 180s" 这种无意义错误。补了 `VIBE_TEST_CHILD_LOG_DIR=<dir>` env：把子进程 stdout/stderr 写到 `<dir>/<pkg>.log` + `RUST_LOG=info`，CI workflow `if: failure()` 时 `::group::` dump 出来。**这是后续所有 CI 问题能查到根因的基础设施**。
3. **VIBE_HEADLESS 在 Xvfb 下挂 lavapipe**：unmapped X11 窗口让 vulkan surface init 出问题；playthrough 录制也需要可见窗口才能让 ffmpeg 抓到内容。加了 `VIBE_TEST_FORCE_VISIBLE=1` env override，CI 默认开启。
4. **`libxkbcommon-x11-0` 不在 runner**：winit 启动时 dlopen 它处理 X11 键盘映射，缺失直接 panic。子进程日志（靠 #2 的基础设施）露出根因，apt 加上即可。

### 最终落地

- 引擎/harness 改动：`crates/vibe_platform/src/desktop.rs`、`crates/vibe_test/src/{lib,client,harness}.rs`（顺手把 lib.rs 超 250 行拆成 client + harness）
- 新增 env：`VIBE_HEADLESS`、`VIBE_TEST_FORCE_VISIBLE`、`VIBE_TEST_RELEASE`、`VIBE_TEST_CHILD_LOG_DIR`
- 新增 `LaunchOptions::visible(bool)` / `LaunchOptions::release(bool)`
- CI 新 job：`vdp-integration`（ui-demo + Xvfb + lavapipe，~4 分钟）
- 新 workflow：`playthrough.yml`（pull_request_target + 三 job 权限隔离）
- 新测试：5 个 demo 各自 `examples/<game>/tests/playthrough.rs`（aoi-demo / flappy-bird / mari0 / tetris / ui-demo，~9-12s 一个），全部用同一套 `GameHarness` + `tokio::sleep` 人速节奏
- AGENTS.md「为游戏编写测试」+「测试与验证」小节同步更新

### 扩到全部 demo（2026-05-16 后续）

Step 4 原计划只录 1 个 demo，后来扩到全部 5 个：

- `playthrough.yml` 的 `record` job 改成 5-way matrix，`fail-fast: false`，1 个 demo 挂不影响其他 4 个
- 每个 matrix entry 上传独立 artifact `playthrough-${pkg}`，文件名 `pr-${PR}-${SHA}-${pkg}.gif`
- `publish` job 改 `if: always()`，下载所有 artifact 拼成**单条**评论里 inline 全部 GIF
- `cleanup` 的 glob `pr-${PR}-*.gif` 无需改动，自动覆盖多文件
- 窗口尺寸不一（aoi 1440×576 / flappy 1280×720 / mari0 1280×960 / tetris 800×700 / ui 1024×640），随 matrix 变量传给 Xvfb + x11grab

新踩的小坑：`engine.simulateInput` 的 `key` 用**短**形式（`"L"` / `"Left"` / `"Space"` ……），不是 winit 长形式（`"KeyL"` / `"ArrowLeft"`），传错被 `Unknown key` 拒掉。已记入 AGENTS.md「VDP 按键命名」段。

### 合并后必须做的两步（不做就没 GIF）

1. 建 orphan 分支：

   ```bash
   git checkout --orphan playthrough-assets
   git rm -rf .
   git commit --allow-empty -m 'init: playthrough assets branch'
   git push -u origin playthrough-assets
   ```

2. Settings → Actions → Workflow permissions 允许 write contents + PR 评论。

完成后第一个改 `crates/vibe_render|vibe_ui|vibe_platform|vibe_test/**` 或 `examples/**` 的 PR 会自动收到 inline 视频评论。

### Recording 黑屏复盘（PR #5，2026-05-16）

合并后 PR #3 自动触发 playthrough，5 张 GIF 全是 YAVG≈16/255 的纯黑。下载 artifact 抽帧测亮度才发现 — 之前每次 CI「成功」也都是黑的，没人打开看过。

**根因**：x11grab 抓 Xvfb 的 root drawable 拿不到 wgpu/lavapipe 的 present 输出。试过加 fluxbox + Xvfb 扩展（RANDR/GLX/COMPOSITE），加 xeyes 对照 — xeyes 截图能看见，游戏窗口在 xwininfo 里**根本不存在**（116 个 X11 children，最大的也才 1022x20 的 fluxbox toolbar），但 wgpu 还在正常 render。

**诊断 smoking gun**：让 ui-demo playthrough 在录制末尾调一次 `engine.screenshot` 让 wgpu 自己 readback PNG，结果 YAVG=102 — wgpu 渲染完全没问题，**X11 display path 才是断的**。

**真正修复**：放弃 x11grab。改用 wgpu texture readback 当 recorder。

- 新增 `ScreenshotPacer`（`crates/vibe_test/src/recorder.rs`）：同步 helper，`pacer.sleep(&mut h, dur)` 替代裸 `tokio::time::sleep`，每个 frame_interval 走 VDP `game.screenshot` 抓一张 PNG。
- CI workflow：删除 x11grab + fluxbox + Xvfb 扩展折腾，只留 Xvfb（winit 启动要 X display）。test 跑完，ffmpeg 把 PNG 序列拼成 MP4。
- 输出从 GIF 改 MP4：MP4 走 GitHub `<video>` tag 有播放控件，文件比 GIF 小 5-10×。
- 文件命名 `pr-${PR}-${SHA}-${pkg}.mp4`，cleanup glob 兼容 `.mp4` + 旧 `.gif`。

**为什么 ScreenshotPacer 不是后台任务**：first attempt 是个跑后台 tokio 任务的 `Recorder`，自己开第二个 VdpClient 连接。本地跑 hang 60s，CI 跑 hang 66 分钟 × 5 个 runner。挖出来 `crates/vibe_debug/src/server.rs:42` 注释清清楚楚 "Accept one connection at a time (simple model for AI tool)" — VDP server 单 client 串行 accept，第二个连接卡在内核 accept queue 里永远收不到。多 client 改造是大手术（要重设计 per-client 响应路由），先放着；ScreenshotPacer 同步在 harness 已有的连接上交错，干净避坑。

**新踩的小坑**：

- ffmpeg `libx264` 不接受奇数维度，scale 时要 `trunc(iw/2)*2:trunc(ih/2)*2`
- artifact 文件大小 `ls -la` 输出在 macOS 上日期列是 "May" 不是 bytes，要 `stat -f '%z'` 看真实字节数
- CI debugging 砸了不少 GitHub Actions 计费，下次类似拿不准的 verify 流程，先缩到单 example + `timeout-minutes` 上限再 push

### MP4 inline 的死路（PR #5 收尾发现）

切到 MP4 后想用 `<video src="raw.githubusercontent.com/.../play.mp4" controls></video>` inline 嵌入 PR 评论。**GitHub 的 sanitizer 把 `<video>` 全部剥光**，只渲染外层 markdown，视频播放器消失。

测了 4 种格式（PR #5 issuecomment-4466794074 / -4466804920）：
- ❌ `<video src controls>` 从 raw.githubusercontent — 被 strip
- ❌ 裸 MP4 URL 单独一行 — 显示成可点链接，不 embed
- ❌ `<img src=mp4>` — 裂图
- ✅ 拖拽上传的 user-attachments URL — 唯一被信任，但 endpoint 没公开 API，CI 拿不到
- ✅ GIF `![]()` inline — 老办法仍然可靠

**最终方案**：workflow 改为同时输出 GIF（inline 预览）+ MP4（评论里贴下载链接，要 scrub 就点开浏览器内置播放器）。

教训：在挑 inline 媒体格式之前，应该先用 4-format test comment 实测 GitHub 渲染再决定。这条已记入 memory `github-pr-video-render`。

详细 fix PR：vibe-ideas/vibe2d#5（待评审）。
