import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  access,
  chmod,
  copyFile,
  cp,
  lstat,
  mkdir,
  readFile,
  readdir,
  readlink,
  writeFile,
} from "node:fs/promises";
import { basename, join, relative, resolve, sep } from "node:path";
import {
  assertChoice,
  parseArgs,
  reportError,
  requireArg,
  resetDirectory,
  run,
} from "./build-utils.mjs";

function xmlEscape(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function infoPlist(version, appName, bundleIdentifier) {
  const escapedVersion = xmlEscape(version);
  const escapedAppName = xmlEscape(appName);
  const escapedBundleIdentifier = xmlEscape(bundleIdentifier);
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "https://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>zh_CN</string>
  <key>CFBundleDisplayName</key>
  <string>${escapedAppName}</string>
  <key>CFBundleExecutable</key>
  <string>corbit</string>
  <key>CFBundleIconFile</key>
  <string>corbit</string>
  <key>CFBundleIdentifier</key>
  <string>${escapedBundleIdentifier}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${escapedAppName}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${escapedVersion}</string>
  <key>CFBundleVersion</key>
  <string>${escapedVersion}</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>Corbit 使用系统事件检查辅助功能状态，并仅在你允许的应用中执行受控操作。</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
`;
}

async function validateDaemonRuntime(runtimeDirectory, expectedVersion, platform, arch) {
  const buildInfoPath = join(runtimeDirectory, "build-info.json");
  const buildInfo = JSON.parse(await readFile(buildInfoPath, "utf8"));
  const expected = {
    product: "Corbit Daemon",
    version: expectedVersion,
    platform,
    arch,
    node: "24.x",
    entrypoint: "src/main.js",
  };
  for (const [key, value] of Object.entries(expected)) {
    if (buildInfo[key] !== value) {
      throw new Error(
        `Daemon runtime ${runtimeDirectory} has invalid ${key}: expected ${value}, found ${buildInfo[key] ?? "missing"}`,
      );
    }
  }
  await access(join(runtimeDirectory, buildInfo.entrypoint));
}

const runtimeMetadataFiles = new Set([
  ".corbit-bundle.json",
  ".corbit-runtime.json",
]);

async function daemonRuntimeDigest(runtimeDirectory) {
  const root = resolve(runtimeDirectory);
  const hash = createHash("sha256");

  async function visit(directory) {
    const entries = await readdir(directory, { withFileTypes: true });
    entries.sort((left, right) => left.name.localeCompare(right.name, "en"));
    for (const entry of entries) {
      if (runtimeMetadataFiles.has(entry.name)) {
        continue;
      }
      const path = join(directory, entry.name);
      const relativePath = relative(root, path).split(sep).join("/");
      const metadata = await lstat(path);
      if (metadata.isSymbolicLink()) {
        hash.update(`link\0${relativePath}\0${await readlink(path)}\0`);
      } else if (metadata.isDirectory()) {
        hash.update(`directory\0${relativePath}\0`);
        await visit(path);
      } else if (metadata.isFile()) {
        hash.update(`file\0${relativePath}\0${metadata.size}\0`);
        for await (const chunk of createReadStream(path)) {
          hash.update(chunk);
        }
        hash.update("\0");
      } else {
        throw new Error(`Unsupported daemon runtime entry: ${path}`);
      }
    }
  }

  await visit(root);
  return `sha256:${hash.digest("hex")}`;
}

async function copyDaemonRuntime(runtimeDirectory, destination, expectedVersion, platform, arch) {
  const source = resolve(runtimeDirectory);
  await validateDaemonRuntime(source, expectedVersion, platform, arch);
  const digest = await daemonRuntimeDigest(source);
  await cp(source, destination, { recursive: true, verbatimSymlinks: true });
  await writeFile(
    join(destination, ".corbit-bundle.json"),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        product: "Corbit Daemon Bundle",
        version: expectedVersion,
        platform,
        arch,
        digest,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
}

async function packageDesktop() {
  const args = parseArgs(process.argv.slice(2));
  const platform = requireArg(args, "platform");
  const arch = requireArg(args, "arch");
  const rustTarget = requireArg(args, "rust-target");
  const profile = requireArg(args, "profile");
  const version = requireArg(args, "version");
  const binary = resolve(requireArg(args, "binary"));
  const assets = resolve(requireArg(args, "assets"));
  const output = resolve(requireArg(args, "output"));
  const signIdentity = args["sign-identity"] ?? "none";
  const appName = args["app-name"] ?? "Corbit";
  const bundleIdentifier =
    args["bundle-identifier"] ?? "com.xiaoxi.corbit.desktop";
  const daemonRuntime = args["daemon-runtime"];
  const daemonRuntimeArm64 = args["daemon-runtime-arm64"];
  const daemonRuntimeX64 = args["daemon-runtime-x64"];
  const daemonVersion = args["daemon-version"];

  if (daemonRuntime || daemonRuntimeArm64 || daemonRuntimeX64) {
    if (!daemonVersion?.trim()) {
      throw new Error("daemon-version is required when packaging a daemon runtime");
    }
    if (arch === "universal") {
      if (daemonRuntime || !daemonRuntimeArm64 || !daemonRuntimeX64) {
        throw new Error(
          "Universal macOS packages require both daemon-runtime-arm64 and daemon-runtime-x64",
        );
      }
    } else if (daemonRuntimeArm64 || daemonRuntimeX64) {
      throw new Error("Architecture-specific daemon runtimes are only valid for universal macOS");
    }
  }

  if (!appName.trim() || basename(appName) !== appName) {
    throw new Error("app-name must be a non-empty file name");
  }
  if (!/^[A-Za-z0-9.-]+$/.test(bundleIdentifier)) {
    throw new Error("bundle-identifier contains unsupported characters");
  }

  assertChoice("platform", platform, ["macos", "linux", "windows"]);
  assertChoice("profile", profile, ["release", "debug"]);
  if (arch === "universal" && platform !== "macos") {
    throw new Error("The universal architecture is supported only for macOS");
  }
  if (!["arm64", "x64", "universal"].includes(arch)) {
    throw new Error(`Unsupported architecture: ${arch}`);
  }

  await access(binary);
  const targetDirectory = await resetDirectory(
    output,
    "desktop",
    `${platform}-${arch}`,
  );

  if (platform === "macos") {
    const appDirectory = join(targetDirectory, `${appName}.app`);
    const contentsDirectory = join(appDirectory, "Contents");
    const executableDirectory = join(contentsDirectory, "MacOS");
    const resourcesDirectory = join(contentsDirectory, "Resources");
    const executable = join(executableDirectory, "corbit");

    await mkdir(executableDirectory, { recursive: true });
    await mkdir(resourcesDirectory, { recursive: true });
    await copyFile(binary, executable);
    await chmod(executable, 0o755);
    await copyFile(
      join(assets, "corbit.icns"),
      join(resourcesDirectory, "corbit.icns"),
    );
    if (daemonRuntime) {
      await copyDaemonRuntime(
        daemonRuntime,
        join(resourcesDirectory, "corbit-daemon"),
        daemonVersion,
        platform,
        arch,
      );
    } else if (arch === "universal") {
      await copyDaemonRuntime(
        daemonRuntimeArm64,
        join(resourcesDirectory, "corbit-daemon", "arm64"),
        daemonVersion,
        platform,
        "arm64",
      );
      await copyDaemonRuntime(
        daemonRuntimeX64,
        join(resourcesDirectory, "corbit-daemon", "x64"),
        daemonVersion,
        platform,
        "x64",
      );
    }
    await writeFile(
      join(contentsDirectory, "Info.plist"),
      infoPlist(version, appName, bundleIdentifier),
      "utf8",
    );

    if (signIdentity !== "none") {
      if (process.platform !== "darwin") {
        throw new Error("Signing a macOS app requires a macOS build host");
      }
      run("/usr/bin/codesign", [
        "--force",
        "--sign",
        signIdentity,
        appDirectory,
      ]);
      run("/usr/bin/codesign", [
        "--verify",
        "--deep",
        "--strict",
        "--verbose=2",
        appDirectory,
      ]);
    }
  } else if (platform === "windows") {
    await copyFile(binary, join(targetDirectory, "Corbit.exe"));
    await copyFile(
      join(assets, "corbit.ico"),
      join(targetDirectory, "corbit.ico"),
    );
    if (daemonRuntime) {
      await copyDaemonRuntime(
        daemonRuntime,
        join(targetDirectory, "corbit-daemon"),
        daemonVersion,
        platform,
        arch,
      );
    }
  } else {
    const executable = join(targetDirectory, "corbit");
    await copyFile(binary, executable);
    await chmod(executable, 0o755);
    await copyFile(
      join(assets, "corbit-app-icon-1024.png"),
      join(targetDirectory, "corbit.png"),
    );
    if (daemonRuntime) {
      await copyDaemonRuntime(
        daemonRuntime,
        join(targetDirectory, "corbit-daemon"),
        daemonVersion,
        platform,
        arch,
      );
    }
  }

  await writeFile(
    join(targetDirectory, "build-info.json"),
    `${JSON.stringify(
      {
        product: "Corbit Desktop",
        version,
        platform,
        arch,
        rustTarget,
        profile,
        sourceBinary: basename(binary),
        channel: profile === "debug" ? "dev" : "release",
        daemonVersion: daemonVersion ?? null,
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  console.log(`Desktop artifact: ${targetDirectory}`);
}

packageDesktop().catch(reportError);
