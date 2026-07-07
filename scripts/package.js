import { exec } from 'child_process';
import { log } from 'console';
import { rm, unlinkSync } from 'fs';
import { mkdirp } from 'mkdirp';
import copy from 'recursive-copy';
import { rimraf } from 'rimraf';


async function main() {
  const release_path = '/tmp/Oyasumi_build/Oyasumi/';
  await rimraf(release_path);
  await mkdirp(release_path);
  await copy('src-core/target/release/OyasumiVR', release_path + 'OyasumiVR', { overwrite: true });
  await copy('src-core/target/release/resources/', release_path + 'resources/', { overwrite: true });
  await execPromise2('./download_cef.sh');
  await copy('cef/', release_path + 'resources/sidecars/cef/', {
    overwrite: true
  });
  try {
    await unlinkSync('/tmp/Oyasumi_build/Oyasumi/resources/manifest.vrmanifest');
  } catch {}
  try {
    await unlinkSync('/tmp/Oyasumi_build/Oyasumi/resources/input');
  } catch {}
  if (process.env.NO_PACKAGE=="1"){
    await rimraf('bin/');
    await copy('/tmp/Oyasumi_build/', 'bin/', {overwrite: true});
    await rimraf('/tmp/Oyasumi_build/Oyasumi/');
	  return;

  }
  console.log('packaging');
  await execPromise('ZSTD_CLEVEL=19 nice -n20 tar -I zstd -cvpf oyasumi-linux.tar.zst Oyasumi/');
  await rimraf('bin/');
  await rimraf('/tmp/Oyasumi_build/Oyasumi/');
  await execPromise('sha512sum oyasumi-linux.tar.zst >> oyasumi-linux.tar.zst.checksum');
  await execPromise('sha256sum oyasumi-linux.tar.zst >> oyasumi-linux.tar.zst.checksum');
  await execPromise('md5sum oyasumi-linux.tar.zst >> oyasumi-linux.tar.zst.checksum');
  await execPromise('sha1sum oyasumi-linux.tar.zst >> oyasumi-linux.tar.zst.checksum');
  try {
    const key = process.env.OYASUMI_GPG_SIGN_KEY;
    if (key) {
      console.log('signing');
      await execPromise('gpg --local-user ' + key + ' -a --detach-sign oyasumi-linux.tar.zst');
      await execPromise(
        'gpg --local-user ' + key + ' -a --detach-sign oyasumi-linux.tar.zst.checksum'
      );
    } else {
      console.warn('GPG_SIGN_KEY env not set release will not be signed');
    }
  } catch {
    console.warn('failed to sign');
  }
  await mkdirp('bin/');
  await copy('/tmp/Oyasumi_build/', 'bin/');
  await rimraf('/tmp/Oyasumi_build');
}
const execPromise = (command) =>
  new Promise((resolve, reject) => {
    exec(command, { cwd: '/tmp/Oyasumi_build/' }, (err, stdout, stderr) => {
      if (err) {
        console.error(err);
        reject(stderr || err);
      } else {
        resolve(stdout);
      }
    });
  });
main().catch((e) => {
  throw e;
});
const execPromise2 = (command) =>
  new Promise((resolve, reject) => {
    exec(command, { }, (err, stdout, stderr) => {
      if (err) {
        console.error(err);
        reject(stderr || err);
      } else {
        resolve(stdout);
      }
    });
  });
main().catch((e) => {
  throw e;
});
