# Contributing to Soroban Upgrade Manager

First off, thank you for taking the time to contribute! 🎉 

Projects like `soroban-upgrade-manager` thrive because of community involvement. Whether you are fixing a typo, optimizing a Rust routine, improving test coverage, or rewriting documentation, your help is incredibly valuable.

Please take a moment to review this document before submitting your first Pull Request (PR) to ensure a smooth review process.

---

## 🗺️ Code of Conduct

By participating in this project, you agree to maintain a respectful, welcoming, and inclusive environment for everyone. Please be professional and constructive in all communications.

---

## 🛠️ Development Setup

To contribute code to this repository, you will need to set up your local development environment.

### Prerequisites
* **Rust**: Version 1.74+ 
* **Stellar CLI**: Version v22+
* **Target**: `wasm32-unknown-unknown` installed via rustup

### Step-by-Step Environment Preparation
1. **Fork and clone** the repository:
   ```bash
   git clone https://github.com
   cd soroban-upgrade-manager
   ```
2. **Add the WASM target** if you haven't already:
   ```bash
   rustup target add wasm32-unknown-unknown
   ```
3. **Verify the build** runs successfully:
   ```bash
   cargo build --target wasm32-unknown-unknown --release --manifest-path contracts/upgrade-manager/Cargo.toml
   ```
4. **Run the existing test suite** to ensure everything passes:
   ```bash
   cargo test
   ```

---

## 💡 How Can I Contribute?

### 1. Reporting Bugs
If you find a security vulnerability, a bug in the timelock calculations, or unexpected execution behavior:
* Open an **Issue** using the bug template.
* Provide a clear description of the bug, the steps to reproduce it, and the expected vs. actual behavior.
* If applicable, include your environment details (Rust version, Stellar CLI version).

### 2. Suggesting Enhancements
Want to add features like vote weight delegation, multi-token governance, or improved logging?
* Open an **Issue** to discuss the feature concept before writing code. This ensures the design aligns with the project's goal of remaining a lean, ultra-secure upgrade layer.

### 3. Submitting Pull Requests
When you are ready to submit code:
1. **Branch Naming**: Create a new branch off `main` using a descriptive name (e.g., `feature/add-emergency-multisig` or `fix/timelock-bounds`).
2. **Write Clean Rust**: Follow idiomatic Rust styles (`cargo fmt` and `cargo clippy` must pass without errors).
3. **Add Tests**: If you add new logic to `governance.rs`, `timelock.rs`, or `lib.rs`, you *must* accompany it with robust unit or integration tests in the test suite.
4. **Update Documentation**: If your changes alter configuration fields or public API functions, update the table in the main `README.md`.

---

## 📑 Pull Request Guidelines

Before pushing the "Create Pull Request" button, double-check that your branch fulfills the following criteria:

* [ ] The code compiles successfully for the `wasm32-unknown-unknown` target.
* [ ] All tests pass via `cargo test`.
* [ ] The code is clean and formatted using `cargo fmt`.
* [ ] Your commit messages are clear, concise, and explain the *why* behind the change.
* [ ] The PR description references any related active issues (e.g., `Closes #12`).

---

## 📜 License

By contributing to `soroban-upgrade-manager`, you agree that your contributions will be licensed under the project's **MIT License**. 
