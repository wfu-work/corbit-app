import { spawn, spawnSync } from "node:child_process";
import { constants } from "node:fs";
import {
  access,
  mkdir,
  open,
  readFile,
  readlink,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { dirname, normalize, resolve } from "node:path";

import {
  assertChoice,
  parseArgs,
  reportError,
  requireArg,
} from "./build-utils.mjs";

const PID_SCHEMA_VERSION = 2;
const LEGACY_PID_SCHEMA_VERSION = 1;
const STOP_TIMEOUT_MS = 2_500;
const KILL_TIMEOUT_MS = 1_000;
const LOCK_TIMEOUT_MS = 10_000;

function comparablePath(path) {
  const normalized = normalize(resolve(path));
  return process.platform === "win32" ? normalized.toLowerCase() : normalized;
}

function pathsMatch(left, right) {
  return comparablePath(left) === comparablePath(right);
}

function delay(milliseconds) {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

function processExists(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") {
      return false;
    }
    if (error?.code === "EPERM") {
      return true;
    }
    throw error;
  }
}

function signalProcess(pid, signal) {
  try {
    process.kill(pid, signal);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") {
      return false;
    }
    throw error;
  }
}

function validatePidRecord(value, pidFile) {
  const schemaVersion = value?.schemaVersion;
  const executable =
    schemaVersion === LEGACY_PID_SCHEMA_VERSION
      ? value?.binary
      : value?.executable;
  if (
    ![LEGACY_PID_SCHEMA_VERSION, PID_SCHEMA_VERSION].includes(schemaVersion) ||
    !Number.isSafeInteger(value.pid) ||
    value.pid <= 0 ||
    !Number.isSafeInteger(value.launcherPid) ||
    value.launcherPid <= 0 ||
    typeof value.binary !== "string" ||
    value.binary.length === 0 ||
    typeof executable !== "string" ||
    executable.length === 0 ||
    typeof value.startedAt !== "string"
  ) {
    throw new Error(`Invalid Corbit development PID file: ${pidFile}`);
  }

  return { ...value, executable };
}

async function readPidRecord(pidFile) {
  try {
    const contents = await readFile(pidFile, "utf8");
    return validatePidRecord(JSON.parse(contents), pidFile);
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null;
    }
    if (error instanceof SyntaxError) {
      throw new Error(`Invalid Corbit development PID file: ${pidFile}`);
    }
    throw error;
  }
}

async function writePidRecord(pidFile, record) {
  await mkdir(dirname(pidFile), { recursive: true });
  const temporaryFile = `${pidFile}.${process.pid}.${Date.now()}.tmp`;

  try {
    await writeFile(temporaryFile, `${JSON.stringify(record, null, 2)}\n`, {
      flag: "wx",
      mode: 0o600,
    });
    await rename(temporaryFile, pidFile);
  } finally {
    await rm(temporaryFile, { force: true });
  }
}

async function pidFileBelongsTo(pidFile, pid, binary) {
  const record = await readPidRecord(pidFile);
  return record?.pid === pid && pathsMatch(record.binary, binary);
}

async function removePidFileIfOwned(pidFile, pid, binary) {
  if (await pidFileBelongsTo(pidFile, pid, binary)) {
    await rm(pidFile, { force: true });
  }
}

function darwinProcessCommand(pid) {
  const result = spawnSync(
    "/bin/ps",
    ["-ww", "-p", String(pid), "-o", "command="],
    { encoding: "utf8", shell: false },
  );

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    return null;
  }
  return result.stdout.trim() || null;
}

async function linuxProcessExecutable(pid) {
  try {
    const executable = await readlink(`/proc/${pid}/exe`);
    return executable.replace(/ \(deleted\)$/, "");
  } catch (error) {
    if (error?.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

function windowsProcessExecutable(pid) {
  const command = [
    "$process = Get-CimInstance Win32_Process -Filter",
    `'ProcessId = ${pid}'`,
    "; if ($process) { [Console]::Out.Write($process.ExecutablePath) }",
  ].join(" ");
  const result = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", command],
    { encoding: "utf8", shell: false },
  );

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `PowerShell could not inspect tracked process ${pid}: ${result.stderr.trim()}`,
    );
  }
  return result.stdout.trim() || null;
}

async function processExecutable(pid) {
  if (process.platform === "darwin") {
    return darwinProcessCommand(pid);
  }
  if (process.platform === "linux") {
    return linuxProcessExecutable(pid);
  }
  if (process.platform === "win32") {
    return windowsProcessExecutable(pid);
  }
  throw new Error(
    `Development process verification is unsupported on ${process.platform}`,
  );
}

async function waitForProcessExit(pid, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!processExists(pid)) {
      return true;
    }
    await delay(50);
  }
  return !processExists(pid);
}

async function stopTrackedProcess(binary, pidFile) {
  const record = await readPidRecord(pidFile);
  if (!record) {
    console.log("[dev] No tracked Corbit debug client is running.");
    return;
  }
  if (!pathsMatch(record.binary, binary)) {
    throw new Error(
      `PID file tracks a different binary (${record.binary}); refusing to stop it.`,
    );
  }
  if (!processExists(record.pid)) {
    await removePidFileIfOwned(pidFile, record.pid, binary);
    console.log(`[dev] Removed stale PID record for process ${record.pid}.`);
    return;
  }

  const actualExecutable = await processExecutable(record.pid);
  if (!actualExecutable || !pathsMatch(actualExecutable, record.executable)) {
    throw new Error(
      `Process ${record.pid} is not the tracked Corbit executable; refusing to stop it.`,
    );
  }

  console.log(`[dev] Stopping tracked Corbit debug client (PID ${record.pid})...`);
  if (!signalProcess(record.pid, "SIGTERM")) {
    await removePidFileIfOwned(pidFile, record.pid, binary);
    return;
  }
  if (!(await waitForProcessExit(record.pid, STOP_TIMEOUT_MS))) {
    const remainingExecutable = await processExecutable(record.pid);
    if (
      !remainingExecutable ||
      !pathsMatch(remainingExecutable, record.executable)
    ) {
      await removePidFileIfOwned(pidFile, record.pid, binary);
      return;
    }
    console.warn(
      `[dev] Process ${record.pid} did not exit after ${STOP_TIMEOUT_MS}ms; forcing it to stop.`,
    );
    if (!signalProcess(record.pid, "SIGKILL")) {
      await removePidFileIfOwned(pidFile, record.pid, binary);
      return;
    }
    if (!(await waitForProcessExit(record.pid, KILL_TIMEOUT_MS))) {
      throw new Error(`Tracked Corbit process ${record.pid} did not stop.`);
    }
  }
  await removePidFileIfOwned(pidFile, record.pid, binary);
}

async function readLockRecord(lockFile) {
  try {
    return JSON.parse(await readFile(lockFile, "utf8"));
  } catch (error) {
    if (error?.code === "ENOENT" || error instanceof SyntaxError) {
      return null;
    }
    throw error;
  }
}

async function removeLockIfOwned(lockFile, token) {
  const record = await readLockRecord(lockFile);
  if (record?.token === token) {
    await rm(lockFile, { force: true });
    return true;
  }
  return false;
}

async function acquireLock(pidFile) {
  const lockFile = `${pidFile}.lock`;
  const token = `${process.pid}-${Date.now()}-${Math.random()}`;
  const deadline = Date.now() + LOCK_TIMEOUT_MS;
  await mkdir(dirname(lockFile), { recursive: true });

  while (Date.now() < deadline) {
    let handle;
    let createdLock = false;
    try {
      handle = await open(lockFile, "wx", 0o600);
      createdLock = true;
      await handle.writeFile(
        `${JSON.stringify({ pid: process.pid, token, createdAt: new Date().toISOString() })}\n`,
      );
      await handle.close();
      handle = undefined;
      return async () => {
        await removeLockIfOwned(lockFile, token);
      };
    } catch (error) {
      if (handle) {
        await handle.close().catch(() => {});
      }
      if (createdLock) {
        await rm(lockFile, { force: true });
        throw error;
      }
      if (error?.code !== "EEXIST") {
        throw error;
      }

      const owner = await readLockRecord(lockFile);
      if (
        Number.isSafeInteger(owner?.pid) &&
        owner.pid > 0 &&
        !processExists(owner.pid)
      ) {
        await removeLockIfOwned(lockFile, owner.token);
        continue;
      }
      await delay(50);
    }
  }

  throw new Error(
    `Timed out waiting for the Corbit development lock: ${lockFile}`,
  );
}

async function withDevelopmentLock(pidFile, operation) {
  const release = await acquireLock(pidFile);
  try {
    return await operation();
  } finally {
    await release();
  }
}

async function spawnTrackedProcess(binary, launchBinary, pidFile) {
  await access(
    launchBinary,
    process.platform === "win32" ? constants.F_OK : constants.X_OK,
  );

  const child = spawn(launchBinary, [], {
    env: process.env,
    stdio: "inherit",
    shell: false,
  });
  const outcome = new Promise((resolveOutcome, rejectOutcome) => {
    child.once("error", rejectOutcome);
    child.once("exit", (code, signal) => resolveOutcome({ code, signal }));
  });
  await new Promise((resolveSpawn, rejectSpawn) => {
    child.once("spawn", resolveSpawn);
    child.once("error", rejectSpawn);
  });

  const record = {
    schemaVersion: PID_SCHEMA_VERSION,
    pid: child.pid,
    launcherPid: process.pid,
    binary,
    executable: launchBinary,
    startedAt: new Date().toISOString(),
  };

  try {
    await writePidRecord(pidFile, record);
  } catch (error) {
    child.kill("SIGTERM");
    throw error;
  }

  console.log(`[dev] Started Corbit debug client (PID ${child.pid}).`);
  return { child, outcome };
}

async function runTrackedProcess(binary, launchBinary, pidFile) {
  let tracked;
  await withDevelopmentLock(pidFile, async () => {
    await stopTrackedProcess(binary, pidFile);
    tracked = await spawnTrackedProcess(binary, launchBinary, pidFile);
  });

  let forwardedSignal = null;
  const signals =
    process.platform === "win32"
      ? ["SIGINT", "SIGTERM"]
      : ["SIGHUP", "SIGINT", "SIGTERM"];
  const handlers = new Map(
    signals.map((signal) => [
      signal,
      () => {
        forwardedSignal = signal;
        if (processExists(tracked.child.pid)) {
          tracked.child.kill(signal);
        }
      },
    ]),
  );
  for (const [signal, handler] of handlers) {
    process.on(signal, handler);
  }

  const { code, signal } = await tracked.outcome;
  for (const [registeredSignal, handler] of handlers) {
    process.off(registeredSignal, handler);
  }
  await removePidFileIfOwned(pidFile, tracked.child.pid, binary);

  if (Number.isInteger(code)) {
    process.exitCode = code;
  } else if (forwardedSignal) {
    process.exitCode = { SIGHUP: 129, SIGINT: 130, SIGTERM: 143 }[
      forwardedSignal
    ];
  } else if (!["SIGHUP", "SIGINT", "SIGTERM"].includes(signal)) {
    process.exitCode = 1;
  }
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const action = requireArg(args, "action");
  assertChoice("action", action, ["run", "stop"]);
  const binary = resolve(requireArg(args, "binary"));
  const pidFile = resolve(requireArg(args, "pid-file"));

  if (action === "run") {
    const launchBinary = resolve(args["launch-binary"] ?? binary);
    await runTrackedProcess(binary, launchBinary, pidFile);
    return;
  }
  await withDevelopmentLock(pidFile, () => stopTrackedProcess(binary, pidFile));
}

main().catch(reportError);
