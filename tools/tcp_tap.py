#!/usr/bin/env python3
"""A transparent TCP recorder for the leak probe.

Usage: tcp_tap.py <listen-port> <target-port> <output-directory>

Everything that passes between a client and the target is copied byte for byte into one file per connection and
direction (conn<N>.c2s and conn<N>.s2c), exactly as a packet capture would see it above TCP. It is the user-space
counterpart of tcpdump on the loopback interface and needs no privileges.
"""
import asyncio
import itertools
import os
import sys


async def main(listen_port, target_port, outdir):
    os.makedirs(outdir, exist_ok=True)
    counter = itertools.count(1)

    async def pump(reader, writer, path):
        with open(path, "ab") as out:
            while True:
                data = await reader.read(65536)
                if not data:
                    break
                out.write(data)
                out.flush()
                writer.write(data)
                await writer.drain()
        writer.close()

    async def handle(client_reader, client_writer):
        n = next(counter)
        try:
            target_reader, target_writer = await asyncio.open_connection("127.0.0.1", target_port)
        except OSError:
            client_writer.close()
            return
        await asyncio.gather(
            pump(client_reader, target_writer, os.path.join(outdir, "conn%d.c2s" % n)),
            pump(target_reader, client_writer, os.path.join(outdir, "conn%d.s2c" % n)),
            return_exceptions=True,
        )

    server = await asyncio.start_server(handle, "127.0.0.1", listen_port)
    print("ready", flush=True)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    try:
        asyncio.run(main(int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]))
    except KeyboardInterrupt:
        pass
