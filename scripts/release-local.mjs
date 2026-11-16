import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { readReleaseConfig, readReleaseAssets, verifyBuildSource, repository } from './release-artifacts.mjs';
import { createGitHubClient, inspectRemote, assertReleaseSource, publishRelease } from './release-github.mjs';

const root = resolve(fileURLToPath(new URL('../', import.meta.url)));

function git(args) {
  const result = spawnSync('git', args, { cwd: root, encoding: 'utf8', windowsHide: true });
  if (result.status !== 0) throw new Error('git ' + args[0] + ' failed');
  return result.stdout.trim();
}

function cleanCommit() {
  if (git(['status', '--porcelain'])) throw new Error('Commit the release changes before building; the working tree must be clean');
  return git(['rev-parse', 'HEAD']);
}

export function signingEnvironment(config, env = process.env, home = homedir()) {
  const result = { ...env };
  if (!result.TAURI_SIGNING_PRIVATE_KEY) {
    const key = join(home, '.tauri', 'motion-studio-release', 'updater.key');
    if (!existsSync(key) || !existsSync(key + '.pub')) {
      throw new Error('Set TAURI_SIGNING_PRIVATE_KEY or restore the existing updater key under ~/.tauri/motion-studio-release/updater.key');
    }
    if (readFileSync(key + '.pub', 'utf8').trim() !== config.publicKey) throw new Error('Local signing public key differs from the application');
    result.TAURI_SIGNING_PRIVATE_KEY = key;
  }
  result.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= '';
  return result;
}

function runNode(args, env = process.env) {
  const result = spawnSync(process.execPath, args, { cwd: root, env, stdio: 'inherit', windowsHide: true });
  if (result.error || result.status !== 0) throw new Error('Release command failed: ' + args[0]);
}

export async function main(args = process.argv.slice(2)) {
  if (args.includes('--help')) {
    console.log('npm run release:local                  Build, sign, verify, and publish\n'
      + 'npm run release:local -- --check        Check local tools, signing key, and GitHub access only\n'
      + 'npm run release:local -- --publish-only Resume uploading the verified build from this commit');
    return;
  }
  if (args.length > 1 || args.some(arg => !['--check', '--publish-only'].includes(arg))) throw new Error('Unknown option; use --help');
  if (process.platform !== 'win32') throw new Error('Build Windows releases on Windows');
  const config = readReleaseConfig(root);
  if (!config.notes) throw new Error('Write docs/releases/' + config.tag + '.md first');
  const origin = git(['remote', 'get-url', 'origin']);
  if (!['https://github.com/' + repository, 'https://github.com/' + repository + '.git', 'git@github.com:' + repository + '.git'].includes(origin)) {
    throw new Error('origin must point to ' + repository);
  }
  const publishOnly = args.includes('--publish-only');
  const buildEnv = publishOnly ? process.env : signingEnvironment(config);
  const tauri = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
  const npmCli = process.env.npm_execpath;
  if (!publishOnly && (!npmCli || !existsSync(npmCli))) throw new Error('Run this command through npm run release:local');
  const cargo = spawnSync('cargo', ['--version'], { cwd: root, encoding: 'utf8', windowsHide: true });
  if (!publishOnly && cargo.status !== 0) throw new Error('Install the Rust toolchain before building');
  const api = createGitHubClient(root);
  const repositoryInfo = await api('/repos/' + repository);
  if (!repositoryInfo.permissions?.push) throw new Error('GitHub credentials do not have repository write access');
  const remote = await inspectRemote(api, config.tag);
  if (args.includes('--check')) {
    console.log('Local build prerequisites ready: Node ' + process.version + ', ' + cargo.stdout.trim());
    console.log('Existing signing key available; GitHub repository write access verified.');
    console.log(remote.release && !remote.release.draft
      ? config.tag + ' is already published. Bump the version and commit/push before the next release.'
      : 'Before publishing, commit and push the release source to origin/main.');
    return;
  }
  const commit = cleanCommit();
  assertReleaseSource(remote, commit);
  if (!publishOnly) {
    if (remote.release) throw new Error('This version already has a release; use a new version or --publish-only for an interrupted local upload');
    if (existsSync(config.directory) || existsSync(config.sourceFile)) {
      throw new Error('Local release artifacts already exist; use --publish-only to resume the original build');
    }
    console.log('Building ' + config.tag + ' from ' + commit + ' using the existing local Cargo cache.');
    runNode([npmCli, 'ci']);
    runNode([tauri, 'build', '--ci', '--bundles', 'nsis', '--config', 'src-tauri/tauri.release.conf.json', '--', '--locked'], buildEnv);
    if (cleanCommit() !== commit) throw new Error('Source changed during the build');
    runNode(['scripts/package-update.mjs']);
    const files = readReleaseAssets(config);
    writeFileSync(config.sourceFile, JSON.stringify({ version: config.version, commit, node: process.version,
      cargo: cargo.stdout.trim(), sha256: Object.fromEntries([...files].map(([name, file]) => [name, file.digest])),
    }, null, 2) + '\n');
  }
  const files = readReleaseAssets(config);
  verifyBuildSource(config, files, commit);
  if (cleanCommit() !== commit) throw new Error('Source changed before publication');
  await publishRelease({ api, config, files, commit });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch(error => {
    console.error(error.message);
    console.error('If a signed build was already prepared, rerun with --publish-only from the same clean commit.');
    process.exitCode = 1;
  });
}
