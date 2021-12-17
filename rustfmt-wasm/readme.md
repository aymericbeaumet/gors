# rustfmt-wasm

This crate lives outside of the Cargo workpace because it requires the unstable
Rust toolchain. It is published to npm.

## Build

```
docker build --platform=linux/amd64 -t rustfmt-wasm .
```

## Publish

```
docker run --platform=linux/amd64 --rm -it rustfmt-wasm
$ npm login
$ npm publish --access=public ./pkg
```

## Development

```
rm -rf ./pkg && docker cp $(docker create --rm rustfmt-wasm):/root/pkg ./pkg
```
