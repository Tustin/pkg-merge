IMAGE := pkg-merge-dev
CARGO_VOLUME := pkg-merge-cargo
TARGET_VOLUME := pkg-merge-target

DOCKER_RUN := docker run --rm \
	-v "$(CURDIR)":/work \
	-v $(CARGO_VOLUME):/usr/local/cargo/registry \
	-v $(TARGET_VOLUME):/target \
	-e CARGO_TARGET_DIR=/target \
	-w /work $(IMAGE)

.PHONY: image test fmt clippy check release-check clean-volumes clean

image:
	docker build -f .devcontainer/Dockerfile -t $(IMAGE) .

test: image
	$(DOCKER_RUN) cargo test --all

fmt: image
	$(DOCKER_RUN) cargo fmt --all -- --check

clippy: image
	$(DOCKER_RUN) cargo clippy --all-targets -- -D warnings

check: image
	$(DOCKER_RUN) cargo check --all-targets

# Comprueba que el perfil release compila (el .exe real lo genera GitHub Actions).
release-check: image
	$(DOCKER_RUN) cargo build --release

clean-volumes:
	docker volume rm $(CARGO_VOLUME) $(TARGET_VOLUME)

clean:
	$(DOCKER_RUN) cargo clean
