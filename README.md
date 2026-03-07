# aitrans - AI 实时语音翻译

一款基于 [Tauri 2](https://v2.tauri.app/) + [SvelteKit](https://svelte.dev/) + [Rust](https://www.rust-lang.org/) 的桌面端 **AI 实时语音翻译** 应用。捕获系统音频或麦克风输入，通过阿里云百炼 (DashScope) 的 `gummy-realtime-v1` 模型进行实时语音识别与翻译，并以透明悬浮窗 (Overlay) 的形式显示翻译结果。

## ✨ 功能特性

- **实时语音翻译** — 基于阿里云 DashScope `gummy-realtime-v1` 模型，支持流式 ASR + 同传翻译
- **多语言支持** — 支持中文、英语、日语、韩语、法语、德语、西班牙语、俄语等 20+ 种语言互译
- **系统音频捕获** — 通过 cpal 捕获系统音频输入设备（虚拟声卡 / 麦克风）
- **透明悬浮窗** — 始终置顶、可拖拽、可调整大小的翻译 Overlay 窗口，支持双语/仅译文两种显示模式
- **自定义热词** — 通过 DashScope Vocabulary API 管理热词表，提升专业术语的识别和翻译准确率
- **高度可配置** — 字体、字号、颜色、透明度、最大行数等 Overlay 样式均可自定义
- **自动重连** — WebSocket 连接断开后自动进行指数退避重连（最多 5 次）
- **VAD 静音检测** — 可调节语音活动检测的静音阈值（200ms–6000ms），适应不同语速

## 🏗️ 技术架构

| 层             | 技术栈                                         |
| -------------- | ---------------------------------------------- |
| **前端 UI**    | SvelteKit 5 + TypeScript + Vite                |
| **桌面框架**   | Tauri 2 (Rust)                                 |
| **音频捕获**   | cpal 0.15                                      |
| **语音翻译**   | 阿里云 DashScope gummy-realtime-v1 (WebSocket) |
| **热词管理**   | DashScope REST API (reqwest)                   |
| **异步运行时** | Tokio                                          |

### 项目结构

```
aitrans/
├── src/                          # 前端 (SvelteKit)
│   ├── app.html                  # HTML 模板
│   ├── lib/
│   │   ├── api.ts                # Tauri invoke 封装
│   │   └── types.ts              # TypeScript 类型定义 & 语言对映射
│   └── routes/
│       ├── +page.svelte          # 主设置页面
│       └── overlay/
│           └── +page.svelte      # 翻译 Overlay 悬浮窗
├── src-tauri/                    # 后端 (Rust / Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json           # Tauri 配置
│   ├── src/
│   │   ├── lib.rs                # 应用入口
│   │   ├── commands.rs           # Tauri 命令 (前后端桥接)
│   │   ├── audio/mod.rs          # 音频捕获 (cpal)
│   │   ├── config/mod.rs         # 配置管理 (JSON 持久化)
│   │   ├── gummy/mod.rs          # DashScope Gummy WebSocket 客户端
│   │   ├── hotwords/mod.rs       # 热词表 CRUD (DashScope REST API)
│   │   └── pipeline/mod.rs       # 音频 → ASR → 翻译 管道
│   └── capabilities/
│       └── default.json          # Tauri 权限声明
├── package.json
├── svelte.config.js
├── vite.config.js
└── tsconfig.json
```

## 📋 前置要求

### 通用要求

- **Node.js** >= 18（推荐 20+）
- **Rust** >= 1.75（通过 [rustup](https://rustup.rs/) 安装）
- **阿里云百炼 (DashScope) API Key** — 用于 gummy-realtime-v1 模型的调用，[获取地址](https://dashscope.console.aliyun.com/)

### macOS 额外要求

- **macOS** >= 14.2（Sonoma，因使用了 macOS Private API）
- **Xcode Command Line Tools**：
    ```bash
    xcode-select --install
    ```

### Windows 额外要求

- **Windows** 10/11
- **Visual Studio Build Tools 2022**（含 "C++ 桌面开发" 工作负载）：
    - 下载地址：https://visualstudio.microsoft.com/visual-cpp-build-tools/
    - 安装时勾选 **"使用 C++ 的桌面开发"**
- **WebView2 Runtime**（Windows 10 需要手动安装，Windows 11 已内置）：
    - 下载地址：https://developer.microsoft.com/microsoft-edge/webview2/

## 🚀 编译与运行

### 1. 克隆项目

```bash
git clone https://github.com/everlasting/aitrans.git
cd aitrans
```

### 2. 安装前端依赖

```bash
npm install
```

### 3. 开发模式

```bash
npm run tauri dev
```

这将同时启动 Vite 开发服务器（端口 1420）和 Tauri 桌面窗口，支持前端热更新。

### 4. 构建发布版本

#### macOS

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`，包含：

- `dmg/` — macOS 安装镜像 (.dmg)
- `macos/` — 应用包 (.app)

> **注意**：macOS 构建需要代码签名。开发阶段可使用 `src-tauri/dev-codesign.sh` 进行临时签名，或在系统设置中允许运行未签名应用。

#### Windows

```powershell
npm run tauri build
```

构建产物位于 `src-tauri\target\release\bundle\`，包含：

- `msi/` — Windows 安装包 (.msi)
- `nsis/` — NSIS 安装包 (.exe)

> **提示**：如在 Windows 上遇到 Rust 编译问题，确保已通过 `rustup` 安装了 `stable-x86_64-pc-windows-msvc` 工具链：
>
> ```powershell
> rustup default stable-msvc
> ```

## ⚙️ 使用说明

1. 启动应用后，在 **设置页面** 填入 DashScope API Key
2. 选择音频输入设备（系统虚拟声卡或麦克风）
3. 配置源语言和目标翻译语言
4. 可选：创建/选择热词表以提升专业术语翻译准确度
5. 点击 **开始** 按钮启动实时翻译管道
6. 翻译结果将显示在透明悬浮窗中，可拖拽至屏幕任意位置

## 🛠️ 推荐 IDE 配置

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 📄 许可证

[MIT](LICENSE)
