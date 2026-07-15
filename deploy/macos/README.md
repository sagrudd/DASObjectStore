# macOS per-user service deployment

`user-service.sh` installs the daemon's validated, render-only service plan in
the invoking user's `gui/<uid>` launchd domain. It refuses root/sudo use,
symlinked or foreign-owned deployment paths, and relative daemon paths.

```console
deploy/macos/user-service.sh install \
  --executable /absolute/path/to/dasobjectstored \
  --config /absolute/path/to/daemon.json
deploy/macos/user-service.sh status
deploy/macos/user-service.sh uninstall
```

An update is transactional: the prior plist and loaded service are restored if
launchd rejects the replacement. Uninstall removes only the user-owned plist;
configuration, logs, and persistent state are retained.

Run the isolated acceptance test on macOS with:

```console
DASOBJECTSTORE_CODEX_VALIDATION_ROOT="$HOME/.dasobjectstore-codex-validation" \
  deploy/macos/test-user-service.sh
```

The test substitutes the CLI, daemon, and launchctl processes. It exercises
install, status, replacement rollback, reinstall, and state-preserving
uninstall without registering a real service or using project/user data.
