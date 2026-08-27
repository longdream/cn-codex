# -*- coding: utf-8 -*-
"""Reusable SSH helper: connect to the server and run a command or open a shell."""
import sys
import paramiko

HOST = "47.113.221.244"
USER = "root"
PASSWORD = "49718751L!abcd"


def connect():
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(
        HOST,
        username=USER,
        password=PASSWORD,
        timeout=20,
        allow_agent=False,
        look_for_keys=False,
    )
    return client


def run_command(client, command, timeout=120):
    stdin, stdout, stderr = client.exec_command(command, timeout=timeout)
    out = stdout.read().decode("utf-8", errors="replace")
    err = stderr.read().decode("utf-8", errors="replace")
    code = stdout.channel.recv_exit_status()
    return code, out, err


def main():
    if len(sys.argv) < 2:
        print("usage: python ssh_helper.py <command>  (or 'shell' for interactive)")
        sys.exit(1)

    client = connect()
    cmd = sys.argv[1]

    if cmd == "shell":
        channel = client.invoke_shell()
        channel.settimeout(0.5)
        print("[connected] interactive shell ready. Send 'exit' to quit.")
        try:
            while True:
                try:
                    data = channel.recv(65535)
                    if data:
                        sys.stdout.write(data.decode("utf-8", errors="replace"))
                        sys.stdout.flush()
                except Exception:
                    pass
        except KeyboardInterrupt:
            pass
        channel.close()
    else:
        command = " ".join(sys.argv[1:])
        code, out, err = run_command(client, command)
        sys.stdout.write(out)
        if err:
            sys.stderr.write(err)
        print(f"\n[exit code: {code}]")
    client.close()


if __name__ == "__main__":
    main()
