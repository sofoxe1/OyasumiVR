#!/bin/bash
ls cef
if [ $? -eq 0 ]; then
  echo "cef already downloaded"
	exit 0
fi
build_dir=$PWD
cd /tmp
mkdir oyasumi_build_cef/
cd oyasumi_build_cef/
wget "https://cef-builds.spotifycdn.com/cef_binary_142.0.14%2Bgceaf578%2Bchromium-142.0.7444.163_linux64_minimal.tar.bz2"
echo "826d1397e4ae5a2a26ff17ebbade76f2e99ce13197f525863ca3eb4f17d46477fb821ad5864e3b2fd4df2670519a346a261e308cb95b6c3380fa2249ee7261f5  cef_binary_142.0.14+gceaf578+chromium-142.0.7444.163_linux64_minimal.tar.bz2"|sha512sum  --check --status
if [ $? -ne 0 ]; then
       	echo "failed to verify cef checksum";
	exit -1;
fi
tar -xvf cef_binary_142.0.14+gceaf578+chromium-142.0.7444.163_linux64_minimal.tar.bz2
rm cef_binary_142.0.14+gceaf578+chromium-142.0.7444.163_linux64_minimal.tar.bz2
mv cef_binary_142.0.14+gceaf578+chromium-142.0.7444.163_linux64_minimal cef
cd cef
rm libcef_dll/ WORKSPACE bazel/ cmake/ include/ *.txt *.html *.bazel .bazel* -R
pushd Release
strip *
mv * ..
popd
rm Release/ -R
mv Resources/* .
rm Resources -R
cd ..
mv cef $build_dir/
rm /tmp/oyasumi_build_cef -R



