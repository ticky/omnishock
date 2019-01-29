# This script takes care of testing your crate

set -ex

main() {
    if [ $TRAVIS_OS_NAME = linux ]; then
        docker build -t sdl2-$TARGET:latest ci/docker/$TARGET
    fi

    cross build --target $TARGET
    cross build --target $TARGET --release

    cargo clippy \
        --all-targets \
        --all-features \
        -- \
        -D warnings

    cargo fmt \
        --all \
        -- \
        --check

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
