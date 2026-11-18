#!/usr/bin/env python3
"""
NETAssist - firma del codice Windows con Azure Artifact Signing (ex "Trusted Signing").

Sostituisce res/job.py di RustDesk mantenendo la stessa interfaccia usata da
.github/workflows/flutter-build.yml:

    BASE_URL=<qualsiasi> SECRET_KEY=<segreto> python3 res/job.py sign_files <cartella> [estensioni]
    BASE_URL=<qualsiasi> SECRET_KEY=<segreto> python3 res/job.py sign_one_file <file>

Segreti del repository GitHub da impostare:
  SIGN_BASE_URL   = azure          (qualsiasi valore: serve solo ad attivare i passaggi di firma)
  SIGN_SECRET_KEY = base64 di un JSON come questo (una riga, senza spazi):
      {"tenant_id":"...","client_id":"...","client_secret":"...",
       "endpoint":"https://weu.codesigning.azure.net",
       "account":"netorange-signing","profile":"netorange-public"}

Il base64 evita problemi di virgolette quando il workflow espande il segreto nella shell.
Requisiti sul runner Windows (già presenti su windows-2022): Windows SDK (signtool.exe), .NET 8.
"""
import argparse
import base64
import glob
import json
import logging
import os
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

logging.basicConfig(level=logging.INFO, format="%(levelname)s %(message)s")

DLIB_PACKAGE = "Microsoft.ArtifactSigning.Client"
DLIB_VERSION = os.environ.get("ARTIFACT_SIGNING_DLIB_VERSION", "1.0.146")
TIMESTAMP_URL = "http://timestamp.acs.microsoft.com"
SIGN_EXTENSIONS = [".exe", ".dll", ".sys", ".msi", ".cat", ".ps1", ".psm1", ".cab"]
SKIP_DIR_MARKERS = ["RustDeskPrinterDriver", "usbmmidd_v2"]  # driver già firmati da terzi
BATCH = 40
WORK = os.path.join(tempfile.gettempdir(), "netassist-signing")


def load_secret():
    raw = os.environ.get("SECRET_KEY", "").strip()
    if not raw:
        sys.exit("SECRET_KEY mancante")
    try:
        cfg = json.loads(base64.b64decode(raw).decode("utf-8"))
    except Exception:
        try:
            cfg = json.loads(raw)
        except Exception:
            sys.exit("SECRET_KEY non valido: deve essere il JSON (in base64) con le credenziali Azure")
    for k in ["tenant_id", "client_id", "client_secret", "endpoint", "account", "profile"]:
        if not cfg.get(k):
            sys.exit(f"SECRET_KEY: manca il campo {k}")
    return cfg


def find_signtool():
    cands = sorted(glob.glob(r"C:\Program Files (x86)\Windows Kits\10\bin\10.*\x64\signtool.exe"), reverse=True)
    if not cands:
        sys.exit("signtool.exe non trovato (serve il Windows SDK)")
    return cands[0]


def get_dlib():
    dlib = os.path.join(WORK, "dlib", "bin", "x64", "Azure.CodeSigning.Dlib.dll")
    if os.path.exists(dlib):
        return dlib
    os.makedirs(WORK, exist_ok=True)
    url = f"https://www.nuget.org/api/v2/package/{DLIB_PACKAGE}/{DLIB_VERSION}"
    pkg = os.path.join(WORK, "dlib.nupkg")
    logging.info(f"Scarico {DLIB_PACKAGE} {DLIB_VERSION}")
    urllib.request.urlretrieve(url, pkg)
    with zipfile.ZipFile(pkg) as z:
        z.extractall(os.path.join(WORK, "dlib"))
    if not os.path.exists(dlib):
        sys.exit(f"Azure.CodeSigning.Dlib.dll non trovata nel pacchetto {DLIB_PACKAGE}")
    return dlib


def metadata_file(cfg):
    path = os.path.join(WORK, "metadata.json")
    os.makedirs(WORK, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump({
            "Endpoint": cfg["endpoint"],
            "CodeSigningAccountName": cfg["account"],
            "CertificateProfileName": cfg["profile"],
        }, f)
    return path


def is_signed(signtool, path):
    r = subprocess.run([signtool, "verify", "/pa", "/q", path], capture_output=True)
    return r.returncode == 0


def sign_batch(files, cfg):
    if not files:
        return True
    signtool = find_signtool()
    env = dict(os.environ)
    env.update({"AZURE_TENANT_ID": cfg["tenant_id"], "AZURE_CLIENT_ID": cfg["client_id"], "AZURE_CLIENT_SECRET": cfg["client_secret"]})
    dlib, meta = get_dlib(), metadata_file(cfg)
    ok = True
    for i in range(0, len(files), BATCH):
        chunk = files[i:i + BATCH]
        cmd = [signtool, "sign", "/v", "/fd", "SHA256", "/tr", TIMESTAMP_URL, "/td", "SHA256", "/dlib", dlib, "/dmdf", meta] + chunk
        logging.info(f"Firmo {len(chunk)} file…")
        r = subprocess.run(cmd, env=env)
        if r.returncode != 0:
            logging.error("signtool ha restituito un errore")
            ok = False
    return ok


def collect(dir_path, only_ext=None):
    if only_ext:
        only_ext = ["." + e.lstrip(".") for e in only_ext.split(",")]
    signtool = find_signtool()
    out = []
    for root, _dirs, files in os.walk(dir_path):
        if any(m in root for m in SKIP_DIR_MARKERS):
            continue
        for name in files:
            p = os.path.join(root, name)
            ext = os.path.splitext(name)[1].lower()
            if ext not in SIGN_EXTENSIONS or (only_ext and ext not in only_ext):
                continue
            if is_signed(signtool, p):
                logging.info(f"già firmato: {p}")
                continue
            out.append(p)
    return out


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("sign_files")
    a.add_argument("dir_path")
    a.add_argument("only_ext", nargs="?", default=None)
    b = sub.add_parser("sign_one_file")
    b.add_argument("file_path")
    args = ap.parse_args()
    cfg = load_secret()
    files = collect(args.dir_path, args.only_ext) if args.cmd == "sign_files" else [args.file_path]
    if not sign_batch(files, cfg):
        sys.exit(1)
    logging.info(f"Firmati {len(files)} file")


if __name__ == "__main__":
    main()
