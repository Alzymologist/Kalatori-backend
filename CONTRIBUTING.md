## Preparing development environment

It's possible to mimic to spawn chopsticks instances in parallel for development purposes. 
Chopsticks Dockerfile exposes 4 ports (8000, 8500, 9000, 9500), so you can spawn up to 4 instances of chopsticks and each one of them will look at different RPC (note that those will be different chains).
Note that the RPCs are not real, so the changes made on one chopsticks instance will not affect the others.

1. `cd chopsticks`
2. `docker compose up`, in case you want to just 2 instances edit the docker-compose.yml file
3. start the app with `KALATORI_CONFIG` environment variable pointing to `configs/chopsticks.toml`

## Running tests locally

While having the kalatori app running. You can run the tests locally by running the following command:

```bash
cd tests/kalatori-api-test-suite
yarn
yarn test
```

You can run specific test similarly to the following command:

```bash
cd tests/kalatori-api-test-suite
yarn test -t "should create, repay, and automatically withdraw an order in USDC"
```

## Version Bumping and Release Process

When you make changes that require a new version of the project, follow these steps to bump the version:

1. **Update the Version Number**:
    - Update version in `Cargo.toml`

2. **Update the Changelog**:
    - Run `git cliff <range> --tag <new-version>` to generate the changelog for the new version.
   ```bash
   # For example in my case the origin of main repository marked as main,
   # som main/main is the main branch of the main repository.
   # 2.1.2 is version example.  
    git cliff main/main..HEAD --tag 2.1.2 -p CHANGELOG.md 
   ```
    - Review the changelog to ensure that the description is meaningful

3. **Add version related changes to commit**:
   ```bash
   git add CHANGELOG.md Cargo.toml Cargo.lock
   git commit -m "chore: bump version to 2.1.2"
   git push origin <branch-name>
    ```

4. **Tag the version at main branch**:
    ```bash
    git tag -a v2.1.2 -m "Release version 2.1.2"
    git push origin v2.1.2
    ```

