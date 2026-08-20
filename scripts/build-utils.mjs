import { spawnSync } from "node:child_process";
import { mkdir, rm } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

export function parseArgs(argv) {
  const args = {};

  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];

    if (!flag?.startsWith("--") || value === undefined) {
      throw new Error(`Invalid argument near ${flag ?? "<end>"}`);
    }

    args[flag.slice(2)] = value;
  }

  return args;
}

export function requireArg(args, name) {
  const value = args[name];
  if (!value) {
    throw new Error(`Missing required argument --${name}`);
  }
  return value;
}

export function assertChoice(label, value, choices) {
  if (!choices.includes(value)) {
    throw new Error(
      `${label} must be one of: ${choices.join(", ")}; received ${value}`,
    );
  }
}

export function safeDescendant(root, ...segments) {
  const resolvedRoot = resolve(root);
  const destination = resolve(resolvedRoot, ...segments);
  const relativePath = relative(resolvedRoot, destination);

  if (
    !relativePath ||
    relativePath.startsWith("..") ||
    isAbsolute(relativePath)
  ) {
    throw new Error(`Refusing unsafe output path: ${destination}`);
  }

  return destination;
}

export async function resetDirectory(root, ...segments) {
  const destination = safeDescendant(root, ...segments);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  return destination;
}

export function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, {
    cwd: options.cwd,
    env: options.env ?? process.env,
    stdio: "inherit",
    shell: false,
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `${command} exited with status ${result.status ?? "unknown"}`,
    );
  }
}

export function reportError(error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
