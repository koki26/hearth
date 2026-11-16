---
description: The commands you type on a Fireplace server, in one place.
---

# Operator cheatsheet

Everything here assumes the layout from [Running the servers](RUNNING.md):
the binary, `fireplace.toml` and `data/` in **`/opt/fireplace`**, owned by the
`hearth` user.

{% hint style="danger" %}
**Always run admin commands from `/opt/fireplace`, as `hearth`.**
`data_dir` in `fireplace.toml` is relative, so run from anywhere else the
command looks for accounts in the wrong folder. Your git checkout (`~/hearth`)
is where you *build*; it is not where the server *lives*.
{% endhint %}

## Make the first admin

Works only while nobody is an admin, and only while the Fireplace is running
(it hands a signed link to the server over loopback):

```sh
cd /opt/fireplace && sudo -u hearth ./fireplace admin make-admin <handle>
```

Then sign in on the account site — the **Admin** page appears. From there,
admins make other admins. If the last admin goes, this command works again.

## All admin commands

| Command                                              | Does                                                    |
| ---------------------------------------------------- | ------------------------------------------------------- |
| `./fireplace admin make-admin <handle>`              | makes the first admin                                   |
| `./fireplace admin reset-password <handle>`          | prints a 30-minute reset link to hand to the person     |
| `./fireplace admin test-mail <address>`              | sends a test letter ([Email](SELFHOST-MAIL.md))         |
| `./fireplace admin policy-mail <letter.txt>`         | tells everybody who asked that the terms changed        |
| `./fireplace --help`                                 | lists these                                             |

Each takes an optional config path last; the default is `fireplace.toml` in
the current directory. Prefix every one with
`cd /opt/fireplace && sudo -u hearth`.

## Service

```sh
systemctl status fireplace
journalctl -u fireplace -f            # everything it does
sudo systemctl restart fireplace      # after a new binary
```

## Update

```sh
cd ~/hearth && git pull
cargo build --release -p hearth-fireplace
sudo install -o hearth -g hearth target/release/fireplace /opt/fireplace/
sudo cp -r web /opt/fireplace/ && sudo chown -R hearth:hearth /opt/fireplace/web
sudo systemctl restart fireplace
```

## Troubleshooting

| You see                                   | Why                                                                                   |
| ----------------------------------------- | ------------------------------------------------------------------------------------- |
| `sudo: ./fireplace: command not found`    | you are not in `/opt/fireplace` — `cd` there first                                     |
| no accounts / refuses to run              | wrong directory, so `data/people.json` is not where it looks                           |
| `no admin command called "make-admin"`    | the installed binary is older than the command — rebuild and install it (above)       |
| make-admin refused                        | somebody already is an admin; they promote you on the Admin page                      |

## Read more

* [The official Fireplace on Oracle Cloud](ORACLE.md) — the full setup, backups included
* [Running a Fireplace under GDPR](PRIVACY-OPS.md) — what admins may and must do
