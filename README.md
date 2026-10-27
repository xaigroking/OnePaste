# OnePaste

## First launch

OnePaste is not code-signed yet, so Windows and macOS both ask you to allow it the first time you open it. The same steps are in `READ-ME-FIRST.txt` inside each zip file.

**Windows**: double-click `OnePaste.exe`. If SmartScreen says "Windows protected your PC", click `More info`, then `Run anyway`.

**macOS**:
1. Double-click `OnePaste.app`. A dialog says Apple cannot verify the app. Click `Done`.
2. Open `System Settings` → `Privacy & Security` and scroll down to the `Security` section.
3. Click `Open Anyway` next to the message about `OnePaste`, then confirm (it may ask for your password or Touch ID).
4. If the message says the app "is damaged and can't be opened", run `xattr -cr <path>/OnePaste.app` in Terminal and try again.

## 简体中文

### 首次启动

OnePaste 目前没有代码签名，所以第一次打开时，Windows 和 macOS 都会拦截一次，需要你手动放行。每个 zip 里的 `READ-ME-FIRST.txt` 是同样的步骤（英文）。系统界面请按英文原样找按钮。

**Windows**：双击 `OnePaste.exe`。如果 SmartScreen 弹出 “Windows protected your PC”，点 `More info`，再点 `Run anyway`。

**macOS**：
1. 双击 `OnePaste.app`。弹窗提示 Apple 无法验证这个应用，点 `Done`（完成）。
2. 打开 `System Settings`（系统设置）→ `Privacy & Security`（隐私与安全性），向下滚动到 `Security` 部分。
3. 在关于 `OnePaste` 的提示旁点 `Open Anyway`，然后确认（可能需要输入密码或使用 Touch ID）。
4. 如果提示应用“已损坏，无法打开”，在“终端”里运行 `xattr -cr <路径>/OnePaste.app`，然后再打开一次。

这一步只需要做一次。
