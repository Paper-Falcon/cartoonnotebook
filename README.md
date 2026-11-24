# 动漫档案馆

记录你看过的动漫：封面墙、观看进度、评分、截图，还有一整面会动的壁纸背景。

本机运行，数据全部存在本地 `data/` 目录，不上传任何服务器。

## 下载

Windows x64 安装包：[releases/CartoonNotebook_0.1.0_x64-setup.exe](releases/CartoonNotebook_0.1.0_x64-setup.exe)
（约 6.4 MB，双击安装，不依赖任何运行库；数据同样存在本机）

## 功能

**收藏管理**

- 封面墙浏览，标题搜索（内置 8900+ 条本地索引，支持别名，不联网也能搜）
- 在线补全：接 AniList，输入名字自动拉取封面、简介、集数
- 观看状态（在看 / 看完 / 想看 / 搁置 / 弃番）、评分、进度、看完日期、短评
- 自定义标签，支持批量整理（多选改状态、加标签、删除）
- 按年份 / 题材 / 评分 / 标签筛选

**素材库**

- 截图、动图、视频片段都能收：上传、Ctrl+V 粘贴、拖拽三种方式
- 引用模式（只记路径，适合大文件）与归档模式（复制进库）自由选择
- 缩略图自动生成，视频支持边看边截帧、记住播放进度续播
- 按集分组，灯箱里直接写集号和备注
- 引用的文件被移动后可重新定位

**备份**

- 一键导出：数据库快照 + 全部封面 + 归档素材，单文件 zip
- 整包恢复：导出的 zip 重新导入即可（重启生效，恢复前自动留底）

**手机访问**

- 局域网内扫码配对，手机浏览器打开同一个库，可看可改

**背景壁纸**

- 读取 Wallpaper Engine 创意工坊目录，视频壁纸循环播放当应用背景
- 场景壁纸用预览图做静态背景，薄纱浓度 / 模糊 / 填充方式可调
- 窗口切到后台自动暂停播放

## 技术栈

Tauri 2 + React + TypeScript + SQLite（rusqlite），无外部服务依赖。

## 开发

```bash
npm install
npm run dev          # 前端热更 + 调试窗口
npm run build        # 仅构建前端
npm run app:build    # 打包独立版（exe / NSIS 安装包）
npm run smoke        # 接口冒烟测试（28 项断言）
npm run index        # 重建标题索引
```

打包注意：必须用 `npm run app:build`，直接 `cargo build --release` 出来的是开发模式二进制。

## 目录

```
data/                  所有本地数据（数据库、封面、素材、备份）
src/                   前端（React）
src-tauri/             后端（Rust：HTTP 服务 + SQLite）
scripts/               索引构建、图标生成、快捷方式、冒烟测试
docs/技术方案-v2.md    设计与踩坑记录
```

## 致谢

- [WebWallGL](https://www.npmjs.com/package/webwallgl)（MIT License, © elysia395）——场景壁纸的 WebGL 实时渲染内核，`public/vendor/` 内含其 UMD 构建与许可证副本

## 声明

图标取自动漫截图，仅供个人使用。Wallpaper Engine 壁纸仅读取本机已下载的创意工坊文件。
