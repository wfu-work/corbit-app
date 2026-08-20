import { access, chmod, copyFile, mkdir, writeFile } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
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
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
`;
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
    }
  } else if (platform === "windows") {
    await copyFile(binary, join(targetDirectory, "Corbit.exe"));
    await copyFile(
      join(assets, "corbit.ico"),
      join(targetDirectory, "corbit.ico"),
    );
  } else {
    const executable = join(targetDirectory, "corbit");
    await copyFile(binary, executable);
    await chmod(executable, 0o755);
    await copyFile(
      join(assets, "corbit-app-icon-1024.png"),
      join(targetDirectory, "corbit.png"),
    );
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
      },
      null,
      2,
    )}\n`,
    "utf8",
  );

  console.log(`Desktop artifact: ${targetDirectory}`);
}

packageDesktop().catch(reportError);
