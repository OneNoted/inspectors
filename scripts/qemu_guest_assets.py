#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import tempfile
import textwrap
import time
import urllib.request
from pathlib import Path
from typing import Any

DEFAULT_BASE_URL = "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img"
DEFAULT_QEMU_IMAGE = "qemux/qemu"
PROFILE_TIMEOUTS = {"regression": 600, "product": 2400}
PROFILE_RAM_MB = {"regression": "4096", "product": "8192"}
PROFILE_DISK_GB = {"regression": "20G", "product": "40G"}


def run(cmd: list[str], *, capture: bool = False, check: bool = True, cwd: Path | None = None, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else None,
        env=env,
        text=True,
        capture_output=capture,
        check=check,
    )


def ensure_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path


def http_json(url: str, method: str = "GET", payload: dict[str, Any] | None = None) -> Any:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, method=method, data=data, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=20) as response:
        body = response.read().decode("utf-8")
        return json.loads(body) if body else None


def download_if_missing(url: str, target: Path) -> Path:
    if target.exists():
        return target
    ensure_dir(target.parent)
    with urllib.request.urlopen(url) as response, target.open("wb") as fh:
        shutil.copyfileobj(response, fh)
    return target


def ensure_ssh_keypair(cache_root: Path) -> tuple[Path, str]:
    ssh_dir = ensure_dir(cache_root / "ssh")
    private_key = ssh_dir / "id_ed25519"
    public_key = ssh_dir / "id_ed25519.pub"
    if not private_key.exists():
        run(["ssh-keygen", "-t", "ed25519", "-N", "", "-f", str(private_key)], capture=True)
    return private_key, public_key.read_text().strip()


def regression_user_data(browser_command: str, public_key: str) -> str:
    return textwrap.dedent(
        f"""\
        #cloud-config
        users:
          - default
          - name: ubuntu
            sudo: ALL=(ALL) NOPASSWD:ALL
            shell: /bin/bash
            ssh_authorized_keys:
              - {public_key}
        package_update: true
        packages:
          - xvfb
          - xdotool
          - x11-utils
          - x11-xserver-utils
          - imagemagick
          - curl
        write_files:
          - path: /etc/systemd/system/acu-guest-runtime.service
            permissions: '0644'
            content: |
              [Unit]
              Description=ACU Guest Runtime
              After=network-online.target
              Wants=network-online.target

              [Service]
              ExecStart=/usr/local/bin/acu-guest-runtime --host 0.0.0.0 --port 4001 --browser-command {browser_command}
              Restart=always
              RestartSec=2
              StandardOutput=journal+console
              StandardError=journal+console

              [Install]
              WantedBy=multi-user.target
        runcmd:
          - [ bash, -lc, 'systemctl disable --now ufw || true' ]
          - [ bash, -lc, 'modprobe 9pnet_virtio || true; modprobe 9p || true; mkdir -p /mnt/shared; mount -t 9p -o trans=virtio shared /mnt/shared || true' ]
          - [ bash, -lc, 'install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime' ]
          - [ bash, -lc, 'systemctl daemon-reload && systemctl enable acu-guest-runtime.service && systemctl restart acu-guest-runtime.service' ]
        """
    )


def product_user_data(browser_command: str, public_key: str) -> str:
    return textwrap.dedent(
        f"""\
        #cloud-config
        users:
          - default
          - name: ubuntu
            sudo: ALL=(ALL) NOPASSWD:ALL
            shell: /bin/bash
            ssh_authorized_keys:
              - {public_key}
        package_update: true
        package_reboot_if_required: false
        packages:
          - gdm3
          - gnome-shell
          - gnome-session
          - gnome-session-bin
          - gnome-terminal
          - nautilus
          - gnome-control-center
          - xorg
          - xserver-xorg
          - xserver-xorg-video-all
          - epiphany-browser
          - xdotool
          - x11-utils
          - x11-xserver-utils
          - imagemagick
          - curl
          - xvfb
          - dbus-x11
          - libgtk-4-dev
          - libadwaita-1-dev
          - libjavascriptcoregtk-6.0-dev
          - libwebkitgtk-6.0-dev
        write_files:
          - path: /etc/gdm3/custom.conf
            permissions: '0644'
            content: |
              [daemon]
              WaylandEnable=false
              AutomaticLoginEnable=true
              AutomaticLogin=ubuntu
          - path: /etc/xdg/autostart/acu-guest-runtime.desktop
            permissions: '0644'
            content: |
              [Desktop Entry]
              Type=Application
              Name=ACU Guest Runtime
              Exec=/usr/local/bin/acu-guest-runtime --host 0.0.0.0 --port 4001 --browser-command {browser_command}
              X-GNOME-Autostart-enabled=true
              Terminal=false
        runcmd:
          - [ bash, -lc, 'systemctl disable --now ufw || true' ]
          - [ bash, -lc, 'modprobe 9pnet_virtio || true; modprobe 9p || true; mkdir -p /mnt/shared; mount -t 9p -o trans=virtio shared /mnt/shared || true' ]
          - [ bash, -lc, 'install -m 0755 /mnt/shared/guest-runtime /usr/local/bin/acu-guest-runtime' ]
          - [ bash, -lc, 'ln -sf /usr/bin/epiphany-browser /usr/local/bin/firefox || true' ]
          - [ bash, -lc, 'systemctl set-default graphical.target || true' ]
        power_state:
          mode: reboot
          timeout: 30
          message: Reboot after desktop bootstrap
        """
    )




def resize_qcow2(qemu_image: str, image_path: Path, size: str) -> None:
    run([
        "docker",
        "run",
        "--rm",
        "-v",
        f"{image_path.parent}:/work",
        "--entrypoint",
        "qemu-img",
        qemu_image,
        "resize",
        f"/work/{image_path.name}",
        size,
    ], capture=True)


def build_seed_iso(qemu_image: str, work_dir: Path, user_data: str, meta_data: str) -> Path:
    seed_dir = ensure_dir(work_dir / "seed")
    (seed_dir / "user-data").write_text(user_data)
    (seed_dir / "meta-data").write_text(meta_data)
    seed_iso = work_dir / "seed.iso"
    run([
        "docker",
        "run",
        "--rm",
        "-v",
        f"{seed_dir}:/work",
        "--entrypoint",
        "sh",
        qemu_image,
        "-lc",
        "cd /work && genisoimage -output /work/seed.iso -volid cidata -joliet -rock user-data meta-data >/dev/null 2>&1",
    ])
    generated = seed_dir / "seed.iso"
    shutil.move(str(generated), seed_iso)
    return seed_iso


def inspect_container_ip(container_name: str) -> str:
    result = run([
        "docker",
        "inspect",
        "-f",
        "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
        container_name,
    ], capture=True, check=False)
    if result.returncode != 0:
        return ""
    return result.stdout.strip()


def container_exists(container_name: str) -> bool:
    result = run(["docker", "inspect", container_name], capture=True, check=False)
    return result.returncode == 0


def docker_logs(container_name: str) -> str:
    try:
        return run(["docker", "logs", container_name], capture=True, check=False).stdout
    except Exception as exc:  # pragma: no cover - best effort
        return f"failed to read logs: {exc}"


def maybe_attach_runtime(base_url: str, profile: str, browser_command: str) -> tuple[str, str]:
    session_payload: dict[str, Any]
    if profile == "product":
        session_payload = {
            "provider": "display",
            "display": ":0",
            "width": 1440,
            "height": 900,
            "browser_command": browser_command,
        }
    else:
        session_payload = {
            "provider": "xvfb",
            "width": 1280,
            "height": 720,
            "browser_command": browser_command,
        }
    created = http_json(f"{base_url}/api/sessions", method="POST", payload=session_payload)
    session_id = created["session"]["id"]
    actions = http_json(f"{base_url}/api/sessions/{session_id}/actions")
    action_names = [action["name"] for action in actions.get("actions", [])]
    if not action_names:
        raise RuntimeError(f"attached runtime session {session_id} exposed no actions")
    return session_id, created["session"].get("display") or ""


def graceful_shutdown(base_url: str, session_id: str) -> None:
    try:
        http_json(
            f"{base_url}/api/sessions/{session_id}/actions",
            method="POST",
            payload={"kind": "run_command", "command": "sync; sudo shutdown -h now || shutdown -h now"},
        )
    except Exception:
        pass


def ensure_image(profile: str, cache_root: Path, guest_runtime_binary: Path, qemu_image: str, browser_command: str, base_url: str) -> dict[str, Any]:
    cache_root = cache_root.resolve()
    profile_dir = ensure_dir(cache_root / profile)
    template_image = profile_dir / f"{profile}.qcow2"
    metadata_path = profile_dir / "metadata.json"
    if template_image.exists():
        return {"profile": profile, "image_path": str(template_image), "metadata_path": str(metadata_path), "created": False}

    base_image = download_if_missing(base_url, cache_root / "base" / "ubuntu-noble-cloudimg-amd64.img")
    private_key, public_key = ensure_ssh_keypair(cache_root)
    work_dir = Path(
        tempfile.mkdtemp(prefix=f"acu-qemu-{profile}-", dir=str(ensure_dir(cache_root / "_build")))
    )
    build_image = work_dir / "boot.qcow2"
    shutil.copy2(base_image, build_image)
    resize_qcow2(qemu_image, build_image, PROFILE_DISK_GB[profile])
    shared_dir = ensure_dir(work_dir / "shared")
    shutil.copy2(guest_runtime_binary, shared_dir / "guest-runtime")

    user_data = (
        product_user_data(browser_command, public_key)
        if profile == "product"
        else regression_user_data(browser_command, public_key)
    )
    meta_data = textwrap.dedent(
        f"""\
        instance-id: acu-{profile}
        local-hostname: acu-{profile}
        """
    )
    seed_iso = build_seed_iso(qemu_image, work_dir, user_data, meta_data)

    container_name = f"acu-image-prep-{profile}-{int(time.time())}"
    cmd = [
        "docker",
        "run",
        "-d",
        "--name",
        container_name,
        "--device",
        "/dev/net/tun",
        "--cap-add",
        "NET_ADMIN",
        "-e",
        f"BOOT=/boot.qcow2",
        "-e",
        f"USER_PORTS=4001,8006",
        "-e",
        f"DISK_SIZE={PROFILE_DISK_GB[profile]}",
        "-e",
        f"RAM_SIZE={PROFILE_RAM_MB[profile]}",
        "-e",
        "ARGUMENTS=-drive file=/seed.iso,format=raw,media=cdrom,readonly=on",
        "-v",
        f"{build_image}:/boot.qcow2",
        "-v",
        f"{seed_iso}:/seed.iso:ro",
        "-v",
        f"{shared_dir}:/shared:ro",
    ]
    if os.path.exists("/dev/kvm"):
        cmd.extend(["--device", "/dev/kvm"])
    else:
        cmd.extend(["-e", "KVM=N"])
    cmd.append(qemu_image)
    run(cmd, capture=True)

    logs_path = profile_dir / "prepare.log"
    try:
        deadline = time.time() + PROFILE_TIMEOUTS[profile]
        base_runtime_url = None
        while time.time() < deadline:
            if not container_exists(container_name):
                raise RuntimeError(f"{container_name} exited before guest runtime became reachable")
            ip = inspect_container_ip(container_name)
            if ip:
                base_runtime_url = f"http://{ip}:4001"
                try:
                    health = http_json(f"{base_runtime_url}/health")
                    if health.get("status") == "ok":
                        break
                except Exception:
                    pass
            time.sleep(5)
        else:
            raise TimeoutError(f"timed out waiting for {profile} guest runtime health")

        assert base_runtime_url is not None
        session_id, display = maybe_attach_runtime(base_runtime_url, profile, browser_command)
        graceful_shutdown(base_runtime_url, session_id)
        shutdown_deadline = time.time() + 60
        while time.time() < shutdown_deadline:
            result = run(["docker", "inspect", "-f", "{{.State.Running}}", container_name], capture=True, check=False)
            if result.returncode != 0 or result.stdout.strip() != "true":
                break
            time.sleep(2)
    except Exception:
        ensure_dir(logs_path.parent)
        logs_path.write_text(docker_logs(container_name))
        raise
    finally:
        run(["docker", "rm", "-f", container_name], check=False, capture=True)

    ensure_dir(profile_dir)
    shutil.move(str(build_image), template_image)
    metadata = {
        "profile": profile,
        "image_path": str(template_image),
        "guest_runtime_binary": str(guest_runtime_binary),
        "base_image": str(base_image),
        "built_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "browser_command": browser_command,
        "logs_path": str(logs_path),
        "ssh_private_key": str(private_key),
        "ssh_user": "ubuntu",
    }
    metadata_path.write_text(json.dumps(metadata, indent=2))
    shutil.rmtree(work_dir, ignore_errors=True)
    return {"profile": profile, "image_path": str(template_image), "metadata_path": str(metadata_path), "created": True}


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    ensure_parser = subparsers.add_parser("ensure-image")
    ensure_parser.add_argument("--profile", choices=["product", "regression"], required=True)
    ensure_parser.add_argument("--cache-root", required=True)
    ensure_parser.add_argument("--guest-runtime-binary", required=True)
    ensure_parser.add_argument("--qemu-image", default=DEFAULT_QEMU_IMAGE)
    ensure_parser.add_argument("--browser-command", default="firefox")
    ensure_parser.add_argument("--base-image-url", default=DEFAULT_BASE_URL)

    args = parser.parse_args()
    if args.command == "ensure-image":
        payload = ensure_image(
            profile=args.profile,
            cache_root=Path(args.cache_root).resolve(),
            guest_runtime_binary=Path(args.guest_runtime_binary).resolve(),
            qemu_image=args.qemu_image,
            browser_command=args.browser_command,
            base_url=args.base_image_url,
        )
        print(json.dumps(payload))
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
