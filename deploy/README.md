# Deploy

## Application

```shell
# create user
useradd --system --home-dir /var/lib/shootsh --shell /usr/sbin/nologin shootsh

# create directories for config and db
mkdir -p /etc/shootsh /var/lib/shootsh

# copy application
cp shootsh_ssh /usr/local/bin/
chmod 755 /usr/local/bin/shootsh_ssh

# copy env
cp env.example /etc/shootsh/env

# create key
ssh-keygen -t ed25519 -f /etc/shootsh/ssh_host_ed25519_key -N ""

# chown first
chown -R shootsh:shootsh /etc/shootsh /var/lib/shootsh
chmod 700 /etc/shootsh /var/lib/shootsh
chmod 600 /etc/shootsh/env /etc/shootsh/ssh_host_ed25519_key

# systemd
cp deploy/systemd/shootsh.service /etc/systemd/system/
chmod 644 /etc/systemd/system/shootsh.service

systemctl daemon-reload
systemctl enable --now shootsh-ssh
```

## Web

```shell
# create directory
mkdir -p /var/www/shootsh
chown shootsh:shootsh /var/www/shootsh

# copy template
cp www/index_template.html /var/www/shootsh/

# script
cp www/generate_html.sh /usr/local/bin/generate_shootsh_html.sh
chmod 755 /usr/local/bin/generate_shootsh_html.sh

# systemd
cp deploy/systemd/shootsh-gen-html.service /etc/systemd/system/
cp deploy/systemd/shootsh-gen-html.timer /etc/systemd/system/
chmod 644 /etc/systemd/system/shootsh-gen-html.service /etc/systemd/system/shootsh-gen-html.timer

systemctl daemon-reload
systemctl enable --now shootsh-gen-html.timer
```
