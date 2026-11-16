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

## Make yourself the Fireplace owner

Works only while there is no owner, and best while the Fireplace is running
(it hands a signed link to the server over loopback):

```sh
cd /opt/fireplace && sudo -u hearth ./fireplace admin make-owner <handle>
```

Then sign in on the account site — the **Admin** page appears. There you make
groups (a name and what its members may do), and put people in them. If the
owner deletes their account, this command works again. A Fireplace upgraded
from before owners makes its oldest admin the owner and puts the other admins
in a group called "Admin".

## All admin commands

| Command                                              | Does                                                    |
| ---------------------------------------------------- | ------------------------------------------------------- |
| `./fireplace admin make-owner <handle>`              | makes the Fireplace owner                               |
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
| `no admin command called "make-owner"`    | the installed binary is older than the command — rebuild and install it (above)       |
| make-owner refused                        | there already is an owner; they put you in a group on the Admin page                  |

## Read more

* [Host a Fireplace on a cloud server](ORACLE.md) — the full setup, backups included
* [Running a Fireplace under GDPR](PRIVACY-OPS.md) — what admins may and must do

## Pictures and words

New pictures wait on the **Admin** page until somebody with `review` approves
them. To check them automatically as well, point `picture_check` at any local
command (an NSFW classifier, say): it runs as `<command> <picture.jpg>` and a
non-zero exit refuses the picture.

```toml
[moderation]
review_pictures = true
picture_check = "/opt/fireplace/check-picture"
```

Blocked words: one per line in `/opt/fireplace/data/blocked-words.txt`, read
at start.
