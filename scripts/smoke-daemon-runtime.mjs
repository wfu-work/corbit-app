import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { parseArgs, reportError, requireArg } from "./build-utils.mjs";

const START_TIMEOUT_MS = 15_000;
const STOP_TIMEOUT_MS = 5_000;
const MAX_CAPTURED_OUTPUT = 1024 * 1024;

function expectedHostPlatform() {
  return { darwin: "macos", linux: "linux", win32: "windows" }[
    process.platform
  ];
}

function expectedHostArch() {
  return { arm64: "arm64", x64: "x64" }[process.arch];
}

function appendBounded(current, chunk) {
  const next = current + chunk;
  return next.length <= MAX_CAPTURED_OUTPUT
    ? next
    : next.slice(next.length - MAX_CAPTURED_OUTPUT);
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) {
    return Promise.resolve(true);
  }
  return new Promise((resolveExit) => {
    const timer = setTimeout(() => resolveExit(false), timeoutMs);
    child.once("exit", () => {
      clearTimeout(timer);
      resolveExit(true);
    });
  });
}

async function stopChild(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGTERM");
  if (await waitForExit(child, STOP_TIMEOUT_MS)) return;
  child.kill("SIGKILL");
  await waitForExit(child, STOP_TIMEOUT_MS);
}

function waitForReady(child, output) {
  return new Promise((resolveReady, rejectReady) => {
    let pending = "";
    const timer = setTimeout(() => {
      rejectReady(
        new Error(
          `Daemon did not become ready within ${START_TIMEOUT_MS / 1000}s\n${output()}`,
        ),
      );
    }, START_TIMEOUT_MS);

    const finish = (callback, value) => {
      clearTimeout(timer);
      child.stdout.off("data", onData);
      child.off("exit", onExit);
      callback(value);
    };
    const inspectLine = (line) => {
      try {
        const event = JSON.parse(line);
        if (
          event?.msg === "Corbit Daemon is ready" &&
          Number.isInteger(event.port) &&
          event.port > 0
        ) {
          finish(resolveReady, event.port);
        }
      } catch {
        // Pino is expected to emit JSON, but unrelated native diagnostics may
        // share stdout. They remain available in the bounded failure output.
      }
    };
    const onData = (chunk) => {
      pending += chunk;
      const lines = pending.split(/\r?\n/);
      pending = lines.pop() ?? "";
      for (const line of lines) inspectLine(line);
    };
    const onExit = (code, signal) => {
      finish(
        rejectReady,
        new Error(
          `Daemon exited before readiness (code=${code ?? "none"}, signal=${signal ?? "none"})\n${output()}`,
        ),
      );
    };

    child.stdout.on("data", onData);
    child.once("exit", onExit);
  });
}

async function smokeDaemonRuntime() {
  const args = parseArgs(process.argv.slice(2));
  const runtime = resolve(requireArg(args, "runtime"));
  const expectedVersion = requireArg(args, "version");
  const expectedPlatform = requireArg(args, "platform");
  const expectedArch = requireArg(args, "arch");
  const node = resolve(args.node ?? process.execPath);

  const nodeMajor = Number(process.versions.node.split(".")[0]);
  if (nodeMajor !== 24) {
    throw new Error(
      `Daemon smoke test requires Node.js 24; found ${process.versions.node}`,
    );
  }
  if (
    expectedPlatform !== expectedHostPlatform() ||
    expectedArch !== expectedHostArch()
  ) {
    throw new Error(
      `Cannot execute ${expectedPlatform}/${expectedArch} Daemon runtime on ${expectedHostPlatform()}/${expectedHostArch()}`,
    );
  }

  const entrypoint = join(runtime, "src", "main.js");
  const buildInfoPath = join(runtime, "build-info.json");
  await access(node);
  await access(entrypoint);
  const buildInfo = JSON.parse(await readFile(buildInfoPath, "utf8"));
  const mismatches = [
    ["product", buildInfo.product, "Corbit Daemon"],
    ["version", buildInfo.version, expectedVersion],
    ["platform", buildInfo.platform, expectedPlatform],
    ["arch", buildInfo.arch, expectedArch],
    ["node", buildInfo.node, "24.x"],
    ["entrypoint", buildInfo.entrypoint, "src/main.js"],
  ].filter(([, actual, expected]) => actual !== expected);
  if (mismatches.length > 0) {
    throw new Error(
      mismatches
        .map(
          ([field, actual, expected]) =>
            `${field}: expected ${expected}, got ${actual ?? "<missing>"}`,
        )
        .join("; "),
    );
  }

  const temporaryRoot = await mkdtemp(join(tmpdir(), "corbit-daemon-smoke-"));
  const daemonHome = join(temporaryRoot, "home");
  const token = "corbit-daemon-smoke-token-with-32-characters";
  const ownerId = randomUUID();
  const childEnvironment = { ...process.env };
  for (const name of Object.keys(childEnvironment)) {
    if (name.startsWith("CORBIT_")) delete childEnvironment[name];
  }
  Object.assign(childEnvironment, {
    CORBIT_AUTH_TOKEN: token,
    CORBIT_DAEMON_HOST: "127.0.0.1",
    CORBIT_DAEMON_PORT: "0",
    CORBIT_DESKTOP_OWNER_ID: ownerId,
    CORBIT_HOME: daemonHome,
    CORBIT_LOG_LEVEL: "info",
    CORBIT_TERMINAL_ENABLED: "false",
  });

  let capturedOutput = "";
  const child = spawn(node, [entrypoint], {
    cwd: runtime,
    env: childEnvironment,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    capturedOutput = appendBounded(capturedOutput, chunk);
  });
  child.stderr.on("data", (chunk) => {
    capturedOutput = appendBounded(capturedOutput, chunk);
  });

  try {
    const port = await waitForReady(child, () => capturedOutput);
    const endpoint = `http://127.0.0.1:${port}`;
    const healthResponse = await fetch(`${endpoint}/health`);
    if (!healthResponse.ok) {
      throw new Error(`Health check returned HTTP ${healthResponse.status}`);
    }
    const health = await healthResponse.json();
    if (
      health?.status !== "ok" ||
      health?.version !== expectedVersion ||
      health?.desktopOwner?.id !== ownerId ||
      health?.desktopOwner?.pid !== child.pid
    ) {
      throw new Error("Health check returned an unexpected Daemon identity");
    }

    const infoResponse = await fetch(`${endpoint}/info`, {
      headers: { authorization: `Bearer ${token}` },
    });
    if (!infoResponse.ok) {
      throw new Error(`Authenticated info check returned HTTP ${infoResponse.status}`);
    }
    const info = await infoResponse.json();
    if (info?.version !== expectedVersion) {
      throw new Error(
        `Info endpoint version mismatch: expected ${expectedVersion}, got ${info?.version ?? "<missing>"}`,
      );
    }
    console.log(
      `Verified isolated Daemon runtime: ${runtime} (${expectedPlatform}/${expectedArch}, ${expectedVersion})`,
    );
  } finally {
    await stopChild(child);
    await rm(temporaryRoot, { recursive: true, force: true });
  }
}

smokeDaemonRuntime().catch(reportError);
