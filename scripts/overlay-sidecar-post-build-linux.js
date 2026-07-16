import { mkdirp } from 'mkdirp';
import copy from 'recursive-copy';
import { rimraf } from 'rimraf';

async function main() {
  // const coreSourceFile = 'src-overlay-sidecar-linux/target/production/src-overlay-sidecar-linux';
  // const coreTargetDirectory = 'src-core/target/release/resources/sidecars';
  // await mkdirp(coreTargetDirectory);
  // const coreTargetFile = 'src-core/target/release/resources/sidecars/oyasumivr-overlay-sidecar';
  // await copy(coreSourceFile, coreTargetFile, { overwrite: true });
  // const webSourceDirectory = 'src-overlay-ui/build';
  // const webTargetDirectory = 'src-core/target/release/resources/sidecars/ui';
  // await rimraf(webTargetDirectory);
  // await mkdirp(webTargetDirectory);
  // await copy(webSourceDirectory, webTargetDirectory, { overwrite: true });
}

main().catch((e) => {
  throw e;
});
