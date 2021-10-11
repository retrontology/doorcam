if ! grep -q 'KERNEL=="video0", SYMLINK="video0", TAG+="systemd"' '/lib/udev/rules.d/99-systemd.rules'
then
    echo 'KERNEL=="video0", SYMLINK="video0", TAG+="systemd"' | sudo tee -a /lib/udev/rules.d/99-systemd.rules
fi
sudo cp backlight-permissions.rules /etc/udev/rules.d/backlight-permissions.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
