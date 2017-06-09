#!/bin/sh

# This needs to be located in /usr/lib/systemd/system-sleep/

if [ "$1" = "pre" ]; then
  nc=$(command -v nc)
  if [ -z "$nc" ]; then
    nc=$(command -v netcat)
  fi
  if [ -z "$nc" ]; then
    nc=$(command -v ncat)
  fi
  if [ -z "$nc" ]; then
    echo "No netcat, nc or ncat found. Exiting..."
    exit 1
  fi
  # Currently on arch netcat has a bug where it does not end the connection.
  # Therefore we use a timeout of 3 seconds.
  echo -ne "\x05" | $nc -w 3 -U /run/user/1000/windows-gaming-driver/control.sock
  # We don't know how long windows needs to suspend, so just wait 8 secnods for now,
  # which is "good enough for my case".
  # TODO: Fix this once #67 is fixed
  sleep 8
fi
