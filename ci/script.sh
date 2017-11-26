# This script takes care of testing your crate

set -ex

main() {
    docker build -t sdl2-$TARGET:latest ci/$TARGET/Dockerfile

    cross build --target $TARGET
    cross build --target $TARGET --release

    cargo fmt -- --write-mode=diff

    if [ ! -z $DISABLE_TESTS ]; then
        return
    fi

    cross test --target $TARGET
    cross test --target $TARGET --release
}

# we don't run the "test phase" when doing deploys
if [ -z $TRAVIS_TAG ]; then
    main
fi
