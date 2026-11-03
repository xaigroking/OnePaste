# OnePaste

## First launch

OnePaste is not code-signed yet, so Windows and macOS both ask you to allow it the first time you open it. The same steps are in `READ-ME-FIRST.txt` inside each zip file.

**Windows**: double-click `OnePaste.exe`. If SmartScreen says "Windows protected your PC", click `More info`, then `Run anyway`.

**macOS**:
1. Double-click `OnePaste.app`. A dialog says Apple cannot verify the app. Click `Done`.
2. Open `System Settings` → `Privacy & Security` and scroll down to the `Security` section.
3. Click `Open Anyway` next to the message about `OnePaste`, then confirm (it may ask for your password or Touch ID).
4. If the message says the app "is damaged and can't be opened", run `xattr -cr <path>/OnePaste.app` in Terminal and try again.

After opening, set a separate **OnePaste passphrase for this computer** before choosing a mode. Enter it again after each restart. It is independent of your vault passphrase and system login. You can optionally save a recovery kit from **Settings → Security and privacy**, including in local-only mode.

When upgrading an existing profile, choose **Start fresh** if you do not need the old history or do not have recovery credentials. No old passphrase or recovery kit is required; previous local files are set aside on this computer, and your server and other devices are unchanged. To keep using the old history, you can instead recover with its kit or the vault passphrase for its saved vault record. Older local-only data cannot be unlocked if its only key was in system storage and no independent copy is available; OnePaste does not read that storage.

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

Use a server with Docker that both computers can reach. Docker runs on that server; the desktop app runs on your Windows or macOS computers. Linux is supported here as a server platform, not a desktop platform. You do not need the `tools` download or development tools for this path.

1. **Start one server.** For access across networks, replace `YOUR_SERVER_PUBLIC_IP` below with your server's public IP address. This placeholder is not a running service. Allow inbound TCP port 8443 on your server and network.

   ```
   docker run -d --name onepaste -p 8443:8443 -v onepaste-data:/data -e ONEPASTE_PUBLIC_URL=https://YOUR_SERVER_PUBLIC_IP:8443 ghcr.io/xaigroking/onepaste-server:edge
   ```

2. **Get a connection code.** Run this on the Docker server and copy the complete `onepaste1:` line:

   ```
   docker exec onepaste /onepaste-server connect-code
   ```

   Each code is single-use. Generating a new code invalidates previous unused codes; keep it private. The first-start code is also available in `docker logs onepaste`.
3. **First desktop.** Download the Windows or macOS portable zip from the same release as the server image, unzip it, and follow [First launch](#first-launch) to allow the app to open. Set this computer’s independent OnePaste passphrase, then choose **Connect to my own server**, paste the code, optionally name this device, and click **Next**. For an empty server, choose and confirm a passphrase, click **Next**, optionally save the recovery kit, then click **Done**. If the server already has a vault, enter its original passphrase and click **Join** instead; use **I forgot my passphrase, I have a recovery kit** if needed.
4. **Second desktop.** On the connected first desktop, open **Devices → Add a device → Make a connection code** and copy the new code. On the second desktop, unzip its desktop download, follow **First launch**, set its independent OnePaste passphrase, choose **Connect to my own server**, paste this new code, click **Next**, and enter the same passphrase to **Join**. Both desktops use the same server; do not deploy another server. If the first desktop is unavailable, use the server's `connect-code` command above to get a new code.
5. **Check both directions.** Copy distinctive text on computer A; confirm it appears in B's history and can be copied there. Then copy different text on B and confirm it arrives in A's history and can be copied. A healthy container or running process only checks server availability; it does not establish that two desktops are syncing. This walkthrough is not a claim of verified public-network stability.

For LAN-only use, use the server's LAN address instead. Both computers must stay on the same network; leaving it does not ensure continued sync.

- **Address and TLS**: set `ONEPASTE_PUBLIC_URL` to the address other computers will use, as in the example. The server makes its own certificate on first start and keeps it in `/data`. The connection code carries the certificate's fingerprint, and the app checks the server against that fingerprint, so no public certificate or domain name is needed. Without `ONEPASTE_PUBLIC_URL` the code would hold the container's internal address.
- **Data**: the volume `onepaste-data`. With a folder instead of a named volume (`-v /srv/onepaste:/data`), the folder must be writable by user id 65532.
- **Backup**: stop the container, then copy the volume out: `docker stop onepaste` and `docker run --rm -v onepaste-data:/data:ro -v "$PWD":/backup busybox tar czf /backup/onepaste-backup.tgz -C /data .`. To restore into a new volume: `docker run --rm -v onepaste-data:/data -v "$PWD":/backup busybox sh -c 'tar xzf /backup/onepaste-backup.tgz -C /data && chown -R 65532:65532 /data'`.
- **Upgrade**: `docker pull ghcr.io/xaigroking/onepaste-server:edge`, then `docker stop onepaste`, `docker rm onepaste` and the same `docker run` command again. The volume, and with it the data and the certificate, stays.
- **Health**: the image reports itself healthy while the server accepts connections (`docker ps` shows it).

### With Docker Compose

Download `docker/compose.yaml` and `docker/.env.example` into the same directory (or use the repository's `docker/` directory). Copy `.env.example` to `.env`, set `ONEPASTE_PUBLIC_URL` to the HTTPS address and host port reachable by your desktops, and set `HOST_PORT` to that same host port. `IMAGE` selects a release tag or `edge`; `HOST_PORT` and `IMAGE` are Compose variables, not server settings. Then run from that directory:

```sh
docker compose --env-file .env up -d --wait
docker compose --env-file .env exec server /onepaste-server connect-code
```

Use each connection code once with the desktop joining steps above. The named volume preserves the vault, certificate and encrypted attachments. Upgrade by choosing the new `IMAGE` in `.env`, then running `docker compose --env-file .env pull` and `docker compose --env-file .env up -d --wait`. Keep the same project name and volume; `down -v` deletes the data. The health check uses the server's own command because the image has no shell or curl. If you replace the named volume with a host directory, prepare it as writable by UID/GID 65532 before starting.

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

进入应用后，先设置独立的 **OnePaste 本机口令**，再选择使用模式；每次重启应用需重新输入。它与保险库同步口令、系统登录密码分开。可在 **设置 → 安全与隐私** 中查看和保存恢复套件，仅本地模式也支持；不保存也能继续使用。

升级已有资料时，如果没有恢复凭据或不需要旧历史，可选择**重新开始**，无需旧口令或恢复套件。原有文件会移至这台电脑上的独立目录，服务器和其他设备不受影响。如需继续使用旧历史，也可选择使用恢复套件或本机已缓存保险库记录对应的保险库口令恢复。旧版仅本地资料若唯一密钥存于系统凭据库且没有独立副本，则此版本无法解锁；OnePaste 不读取系统凭据库。
