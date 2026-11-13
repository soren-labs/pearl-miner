#!/usr/bin/env python3
"""Exercise the real daemon/GPU against a local pool. No public shares or user wallets.
Run after a release build: python3 scripts/smoke_test.py [path/to/core]
"""
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import sys
import tempfile
import threading
import time


def main():
    binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else 'target/release/pearl-miner-core').resolve())
    with tempfile.TemporaryDirectory(prefix='pearl-smoke-') as home:
        env = dict(os.environ, PEARL_MINER_HOME=home)
        server = socket.socket()
        server.bind(('127.0.0.1', 0))
        server.listen()
        server.settimeout(1)
        done = threading.Event()
        peers = []

        def pool():
            while not done.is_set():
                try:
                    conn, _ = server.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                peers.append(conn)
                header = '00' * 72 + 'ffff001d'
                conn.sendall((json.dumps({'id': 2, 'result': True}) + '\n' + json.dumps({
                    'method': 'mining.notify', 'params': {'job_id': 'local-test', 'header': header,
                    'target': '01', 'cert_version': 3, 'height': 1}}) + '\n').encode())
        threading.Thread(target=pool, daemon=True).start()
        proc = subprocess.Popen([binary, 'daemon'], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, env=env)
        events = queue.Queue()
        def read():
            for line in proc.stdout:
                events.put(json.loads(line))
        threading.Thread(target=read, daemon=True).start()
        def send(cmd, **args):
            proc.stdin.write(json.dumps(dict(cmd=cmd, **args)) + '\n')
            proc.stdin.flush()
        def wait(predicate, timeout=40):
            end = time.monotonic() + timeout
            while time.monotonic() < end:
                try:
                    event = events.get(timeout=min(1, end - time.monotonic()))
                except queue.Empty:
                    if proc.poll() is not None:
                        raise AssertionError('daemon exited: ' + proc.stderr.read())
                    continue
                if predicate(event):
                    return event
            raise AssertionError('Timed out waiting for daemon event')
        try:
            state = wait(lambda e: e['type'] == 'state_snapshot')
            cfg = state['config']
            cfg.update(pool='custom', custom_host='127.0.0.1', custom_port=server.getsockname()[1],
                       worker='smoke-test', live_market_sync=False, keep_awake=False)
            send('update_config', config=cfg)
            wait(lambda e: e['type'] == 'config_updated')
            send('wallet_create')
            wallet = wait(lambda e: e['type'] == 'wallet_updated')['wallet']
            send('wallet_export', path=str(Path(home) / 'export.json'))
            wait(lambda e: e['type'] == 'wallet_exported')
            assert (Path(home) / 'export.json').stat().st_mode & 0o777 == 0o600
            send('start')
            wait(lambda e: e['type'] == 'error' and 'Back up' in e['message'])
            send('wallet_acknowledge_backup')
            wait(lambda e: e['type'] == 'wallet_updated')
            send('start')
            first = wait(lambda e: e['type'] == 'metrics')
            assert first['intensity_now'] == 100
            assert not first['economics']['earnings_available']
            cfg.update(intensity=25, electricity_usd_per_kwh=0.4)
            send('update_config', config=cfg)
            wait(lambda e: e['type'] == 'config_updated')
            wait(lambda e: e['type'] == 'metrics' and e['intensity_now'] == 25)
            send('wallet_create')
            wait(lambda e: e['type'] == 'error' and 'Stop mining' in e['message'])
            send('stop')
            wait(lambda e: e['type'] == 'status' and e['state'] == 'stopped')
            send('get_state')
            state = wait(lambda e: e['type'] == 'state_snapshot')
            assert not state['mining'] and state['wallet']['address'] == wallet['address']
            send('wallet_create')
            wait(lambda e: e['type'] == 'wallet_updated')
            backups = list((Path(home) / 'wallet-backups').glob('*.json'))
            assert backups
            assert any(json.loads(p.read_text())['address'] == wallet['address'] for p in backups)
            bad = dict(cfg, intensity=0)
            send('update_config', config=bad)
            wait(lambda e: e['type'] == 'error')
            ack = wait(lambda e: e['type'] == 'config_updated')
            assert ack['config']['intensity'] == 25
            send('wallet_acknowledge_backup')
            wait(lambda e: e['type'] == 'wallet_updated')
            send('start')
            wait(lambda e: e['type'] == 'metrics' and e['intensity_now'] == 25)
        finally:
            if proc.poll() is None:
                send('quit')
                try:
                    proc.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                    raise AssertionError('daemon did not shut down promptly')
            done.set()
            for conn in peers:
                conn.close()
            server.close()
        assert proc.returncode == 0
        print("PASS: live GPU intensity, unavailable earnings, backup/export/acknowledgement, wallet lock, stop/restart, invalid-config rollback, quit while mining")


if __name__ == '__main__':
    main()
