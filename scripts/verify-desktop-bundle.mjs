import { access, readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { parseArgs, reportError, requireArg } from "./build-utils.mjs";

function plistValue(plist, key) {
  const pattern = new RegExp(
    `<key>${key}</key>\\s*<string>([^<]*)</string>`,
  );
  return plist.match(pattern)?.[1];
}

async function verifyBundle() {
  const args = parseArgs(process.argv.slice(2));
  const app = resolve(requireArg(args, "app"));
  const expectedName = requireArg(args, "name");
  const expectedBundleIdentifier = requireArg(args, "bundle-identifier");
  const expectedVersion = requireArg(args, "version");
  const expectedDaemonVersion = requireArg(args, "daemon-version");
  const expectedArch = requireArg(args, "arch");

  const infoPlistPath = resolve(app, "Contents", "Info.plist");
  const executablePath = resolve(app, "Contents", "MacOS", "corbit");
  const buildInfoPath = resolve(app, "..", "build-info.json");
  const daemonBuildInfoPath = resolve(
    app,
    "Contents",
    "Resources",
    "corbit-daemon",
    "build-info.json",
  );
  const daemonBundleInfoPath = resolve(
    app,
    "Contents",
    "Resources",
    "corbit-daemon",
    ".corbit-bundle.json",
  );
  await access(infoPlistPath);
  await access(executablePath);
  await access(buildInfoPath);
  await access(daemonBuildInfoPath);
  await access(daemonBundleInfoPath);

  const plist = await readFile(infoPlistPath, "utf8");
  const buildInfo = JSON.parse(await readFile(buildInfoPath, "utf8"));
  const daemonBuildInfo = JSON.parse(await readFile(daemonBuildInfoPath, "utf8"));
  const daemonBundleInfo = JSON.parse(await readFile(daemonBundleInfoPath, "utf8"));
  await access(
    resolve(
      app,
      "Contents",
      "Resources",
      "corbit-daemon",
      "src",
      "main.js",
    ),
  );
  const actualName = plistValue(plist, "CFBundleName");
  const actualBundleIdentifier = plistValue(plist, "CFBundleIdentifier");
  const actualVersion = plistValue(plist, "CFBundleShortVersionString");

  const mismatches = [
    ["CFBundleName", actualName, expectedName],
    ["CFBundleIdentifier", actualBundleIdentifier, expectedBundleIdentifier],
    ["CFBundleShortVersionString", actualVersion, expectedVersion],
    ["build-info product", buildInfo.product, "Corbit Desktop"],
    ["build-info version", buildInfo.version, expectedVersion],
    ["build-info platform", buildInfo.platform, "macos"],
    ["build-info arch", buildInfo.arch, expectedArch],
    ["build-info profile", buildInfo.profile, "debug"],
    ["build-info channel", buildInfo.channel, "dev"],
    ["build-info daemonVersion", buildInfo.daemonVersion, expectedDaemonVersion],
    ["daemon product", daemonBuildInfo.product, "Corbit Daemon"],
    ["daemon version", daemonBuildInfo.version, expectedDaemonVersion],
    ["daemon platform", daemonBuildInfo.platform, "macos"],
    ["daemon arch", daemonBuildInfo.arch, expectedArch],
    ["daemon Node", daemonBuildInfo.node, "24.x"],
    ["daemon entrypoint", daemonBuildInfo.entrypoint, "src/main.js"],
    ["daemon bundle schema", daemonBundleInfo.schemaVersion, 1],
    ["daemon bundle product", daemonBundleInfo.product, "Corbit Daemon Bundle"],
    ["daemon bundle version", daemonBundleInfo.version, expectedDaemonVersion],
    ["daemon bundle platform", daemonBundleInfo.platform, "macos"],
    ["daemon bundle arch", daemonBundleInfo.arch, expectedArch],
  ].filter(([, actual, expected]) => actual !== expected);

  if (!/^sha256:[0-9a-f]{64}$/.test(daemonBundleInfo.digest ?? "")) {
    mismatches.push([
      "daemon bundle digest",
      daemonBundleInfo.digest,
      "sha256:<64 lowercase hex characters>",
    ]);
  }

  if (mismatches.length > 0) {
    throw new Error(
      mismatches
        .map(([field, actual, expected]) => `${field}: expected ${expected}, got ${actual ?? "<missing>"}`)
        .join("; "),
    );
  }

  console.log(`Verified desktop bundle: ${app}`);
}

verifyBundle().catch(reportError);
