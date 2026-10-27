# OnePaste

## First launch

OnePaste is not code-signed yet, so Windows and macOS both ask you to allow it the first time you open it. The same steps are in `READ-ME-FIRST.txt` inside each zip file.

**Windows**: double-click `OnePaste.exe`. If SmartScreen says "Windows protected your PC", click `More info`, then `Run anyway`.

**macOS**:
1. Double-click `OnePaste.app`. A dialog says Apple cannot verify the app. Click `Done`.
2. Open `System Settings` → `Privacy & Security` and scroll down to the `Security` section.
3. Click `Open Anyway` next to the message about `OnePaste`, then confirm (it may ask for your password or Touch ID).
4. If the message says the app "is damaged and can't be opened", run `xattr -cr <path>/OnePaste.app` in Terminal and try again.

## Run your own server

`onepaste-server` is a single program that serves HTTPS with its own certificate; the app pins that certificate, so you do not need a domain name or a certificate authority.

1. Start it, telling it the address other computers will use to reach it (the first start also creates the certificate and the database in the data directory):

   ```
   onepaste-server --data-dir ./onepaste-data --public-url https://192.168.1.20:8443
   ```

   `--listen` sets the address and port it listens on (default `0.0.0.0:8443`). The same settings can be given with the environment variables `ONEPASTE_DATA_DIR`, `ONEPASTE_LISTEN` and `ONEPASTE_PUBLIC_URL`.
2. The **connection code** is printed on the first start, on a line that begins with `onepaste1:`. To get a new one later, run the same program with `connect-code` (a new code replaces the unused older ones):

   ```
   onepaste-server connect-code --data-dir ./onepaste-data --public-url https://192.168.1.20:8443
   ```
3. In the app, choose "Connect to my own server" and paste the connection code. Every further computer needs its own connection code.

### With Docker

Each pre-release also has a server image for 64-bit Intel/AMD and for Arm, at `ghcr.io/xaigroking/onepaste-server` (the tag is the release name, for example `v0.0.1-main.7`; `edge` is the newest automatic pre-release; there is no `latest`). The image has no shell, runs as an unprivileged user, and keeps everything in the volume `/data`.

```
docker run -d --name onepaste -p 8443:8443 -v onepaste-data:/data -e ONEPASTE_PUBLIC_URL=https://192.168.1.20:8443 ghcr.io/xaigroking/onepaste-server:edge
docker logs onepaste
```

- **Connection code**: the line that starts with `onepaste1:` in `docker logs onepaste`. To print a new one later (it replaces the unused older ones): `docker exec onepaste /onepaste-server connect-code`.
- **Address and TLS**: set `ONEPASTE_PUBLIC_URL` to the address other computers will use, as in the example. The server makes its own certificate on first start and keeps it in `/data`. The connection code carries the certificate's fingerprint, and the app checks the server against that fingerprint, so no public certificate or domain name is needed. Without `ONEPASTE_PUBLIC_URL` the code would hold the container's internal address.
- **Data**: the volume `onepaste-data`. With a folder instead of a named volume (`-v /srv/onepaste:/data`), the folder must be writable by user id 65532.
- **Backup**: stop the container, then copy the volume out: `docker stop onepaste` and `docker run --rm -v onepaste-data:/data:ro -v "$PWD":/backup busybox tar czf /backup/onepaste-backup.tgz -C /data .`. To restore into a new volume: `docker run --rm -v onepaste-data:/data -v "$PWD":/backup busybox sh -c 'tar xzf /backup/onepaste-backup.tgz -C /data && chown -R 65532:65532 /data'`.
- **Upgrade**: `docker pull ghcr.io/xaigroking/onepaste-server:edge`, then `docker stop onepaste`, `docker rm onepaste` and the same `docker run` command again. The volume, and with it the data and the certificate, stays.
- **Health**: the image reports itself healthy while the server accepts connections (`docker ps` shows it).

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

