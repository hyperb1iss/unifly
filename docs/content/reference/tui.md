+++
title = "TUI Dashboard"
description = "11-screen terminal dashboard and network client, with the keys for every screen"
weight = 2
+++

The `unifly tui` subcommand launches a terminal client for your UniFi network. Eleven screens cover live monitoring (bandwidth charts, events, WiFi observability) and day-to-day management: restarting and adopting devices, blocking clients, editing networks, toggling and reordering firewall rules, and managing SSIDs.

## Launch

```bash
unifly tui                   # Launch with default profile
unifly tui -p office         # Use a specific profile
unifly tui -k                # Accept self-signed TLS certs
unifly tui -v                # Verbose logging to temp directory
unifly tui --demo-fleet      # Synthetic offline fleet, no controller needed
```

## Screens

Navigate with number keys `1`-`9`, `Tab`/`Shift+Tab`, a click or tap on the tab bar, or `,` for Settings:

| Key | Screen         | When to Use                                                                |
| --- | -------------- | -------------------------------------------------------------------------- |
| `1` | **Dashboard**  | At-a-glance health check and alarm count. Start here.                      |
| `2` | **Devices**    | Inspecting, restarting, upgrading, or adopting a device; switch port work  |
| `3` | **Clients**    | Finding a client, blocking or kicking it, fixed IPs, guest access          |
| `4` | **Networks**   | Reviewing VLANs, creating, editing, and deleting networks                  |
| `5` | **Firewall**   | Auditing, toggling, deleting, and reordering policies; zones, ACL, and NAT |
| `6` | **Topology**   | Understanding physical network layout, tracing uplink paths                |
| `7` | **Events**     | Troubleshooting from the live feed, older history, and active alarms       |
| `8` | **Stats**      | Historical analysis, bandwidth trends, DPI app breakdown                   |
| `9` | **WiFi**       | AP health, SSID management, channel overlap, client experience, roaming    |
| `,` | **Settings**   | Editing and switching profiles, testing a connection, changing the theme   |
|     | **Onboarding** | Setup wizard, shown when the launch has no profile it can connect with     |

## Key Bindings

Press `?` (or `F1`) on any screen for the keys that work there. The overlay lists the active screen's keys for the mode it is in (list, open detail, form, held policy move) beside the global ones and the navigation every list shares. The status bar offers only the actions that work for the selected row (see Status Bar and Toolbar below). The overlay follows the same rule: it lists `/` only on screens where search filters something, and a description too long for its column wraps onto the next row.

### Global

| Key                                   | Action                                                            |
| ------------------------------------- | ----------------------------------------------------------------- |
| `1`-`9`, `,`                          | Jump to screen, or open Settings                                  |
| `Tab` / `Shift+Tab`                   | Next / previous screen, on every screen                           |
| `j` / `k` / `Up` / `Down`             | Move up / down in a list                                          |
| `g` / `G` / `Home` / `End`            | Jump to top / bottom                                              |
| `Ctrl+d` / `Ctrl+u` / `PgDn` / `PgUp` | Page down / up                                                    |
| `h` / `l` / `Left` / `Right`          | Step the screen's sub-tabs, filter, or period                     |
| `Enter`                               | Open or toggle the detail of the selected row                     |
| `Esc`                                 | Close, unfilter, or go back (order below)                         |
| `/`                                   | Search (Devices, Clients, Events, WiFi)                           |
| `Ctrl+r`                              | Refresh now, or retry the connection while disconnected           |
| `n`                                   | Notification history                                              |
| `?` / `F1`                            | Help overlay for the screen and mode on show                      |
| `a`                                   | About                                                             |
| `Ctrl+l`                              | Redraw the screen                                                 |
| `q` / `Ctrl+c`                        | Quit. `Ctrl+c` quits from every state, dialogs and forms included |

`Esc` closes one thing per press, innermost first:

1. Whatever the screen has open: a detail panel, a held policy move, the WiFi AP focus, the Topology selection, the Alarms view.
2. A warning or error toast.
3. A kept search filter.
4. The screen itself, going back once to the screen you came from.

While a field has the keyboard (the search bar, the Networks form, a prompt, Setup, Settings), keys type into it and no shortcut fires, so `q` and digits are text. `?` is text there too, which is why `F1` also opens help. Pasted text goes to that field as one piece, and a newline in it never submits; a paste anywhere else is dropped, so it cannot run as a burst of shortcut keys. A `Ctrl` or `Alt` chord never acts as its plain letter.

While a form is open (the Networks form, Setup, Settings), the status bar shows only `F1 help` and `^c quit`, and the help overlay lists only the keys that still work. `Ctrl+r` does nothing there, so a refresh cannot swap the record being edited. Held down anywhere else, `Ctrl+r` is one refresh, not a queue of them.

### Status Bar and Toolbar

The status bar is the last row of the screen and the one place a screen's key hints are drawn. From the left it shows the connection state, then `│`, then the actions the selected row or the screen can take right now, such as `Enter detail  b block  x kick  I fixed IP  F forget` on Clients, with `Esc close` last while a detail is open. As many actions as fit are shown, most important first. Against the right edge sit `/ search` on screens that search, `^r retry` while disconnected, and `? help`, and after them the sponsor button on terminals 100 columns or wider.

Navigation that every list shares (`j`/`k`, `h`/`l`, `g`/`G`, paging) is not in the status bar, and neither are `Ctrl+r` to refresh, `,` for Settings, and `q` to quit. The help overlay lists them all.

The first row inside a list screen is its toolbar. Sub-tabs and filters sit on the left, every tab keeps its width, and the active one is bold and underlined in the accent color. State that describes the view sits against the right edge as a chip that shows its own key, such as `s sort name ↑` on Devices and Clients, and is left out when the row has no room for it. Row counts are in the screen title.

### Mouse and Touch

Tabs, list rows, key hints, dialogs, and dashboard panels take a click, so the TUI runs on a touchscreen kiosk without a keyboard. On a touchscreen, taps work wherever the terminal reports them as mouse clicks.

| Target               | Click or tap                                                                                                                                                                                                                                                                             |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Tabs**             | Switches to the tab: the screen tabs along the bottom, the Devices and Clients filters, the Devices detail tabs, the Firewall and WiFi sub-tabs, the Stats periods, and the Events categories                                                                                            |
| **Table rows**       | The first tap selects a row, and a second tap does what `Enter` does there: opens a device, client, or event, or toggles a network, firewall, or WiFi detail                                                                                                                             |
| **Open details**     | A tap on another row moves an open device or client detail to it. The device detail keeps its tab, so stepping from switch to switch stays on their ports. A tap on a port or client row moves its cursor                                                                                |
| **Key hints**        | Every hint with a key is a button that presses it: `R restart`, `Esc close`, and the status bar's `/ search`, `^r retry`, and `? help`. Toolbar chips that show a key are buttons too: `s sort`, `Space ● live`, `v alarms`. A form hint such as `←/→ pick` is plain text                |
| **Dialogs**          | A confirmation takes `y` and `n` taps, and a tap outside it answers no. A prompt cancels on a tap outside it. Help, About, and the notification history close on a tap outside them                                                                                                      |
| **Notifications**    | A tap dismisses the toast                                                                                                                                                                                                                                                                |
| **Dashboard panels** | A tap focuses the panel and opens its screen: WAN Traffic opens Stats, Gateway opens the gateway, Connectivity and Capacity open Devices, and Networks, WiFi, Top Clients, and Recent Events open their screens. A tapped access point or client row opens that device or client instead |
| **Topology**         | A tap selects the device under it, and a second tap opens it on the Devices screen                                                                                                                                                                                                       |
| **Form fields**      | In the Networks form a tap focuses a field and flips an on/off field. Settings and Setup fields take taps too                                                                                                                                                                            |
| **WiFi AP focus**    | The `focus ap-atrium ✕` chip on the toolbar clears the focus                                                                                                                                                                                                                             |

The wheel moves the selection in lists, and an open device or client detail follows it; over a device's Ports or Clients tab it moves that list instead. On Events it moves through the feed, pausing a live feed first when you scroll back. On Topology the wheel zooms on the pointer and a drag pans the map. In the Networks form it steps through the fields. In Settings it scrolls the form, and walks the theme picker when that is open, previewing each theme; a tap outside the picker cancels it.

The TUI captures the mouse, so hold `Shift` while dragging to select text with the terminal.

Typing still needs a keyboard: search, prompts, the Networks form, the Settings fields, and the setup wizard.

## Feedback

Everything the TUI does on the controller reports back the same way:

- **Toasts** stack in the top-right corner under the toolbar row, up to four at a time, and wrap long text instead of cutting it. One never replaces another, so an error is not wiped by the success that lands after it. The newest toast is always drawn: when the stack is taller than the screen the older ones give way, and when a fifth arrives the oldest one that is not a warning or an error goes first, never the one that just arrived. Notes and successes clear after 3 seconds, warnings after 8, errors after 10. `Esc` dismisses warnings and errors, and a tap dismisses any toast.
- **Confirmations** come before anything disruptive (a restart, a block, a delete, turning a rule on or off). The dialog names the target and says what yes costs. `y` confirms; `n` or `Esc` cancels. A question that arrives late, from a check that ran in the background, never replaces a dialog you are reading and never opens over a prompt or the search bar. It is dropped with a notice to press the key again.
- **Prompts** ask for a single value, such as a fixed IP, a port name, or a passphrase. `Enter` saves and `Esc` cancels. The field takes the line-editing keys and a paste. A passphrase is masked, and `Ctrl+t` shows or hides it. A value that cannot be used reopens the prompt on what you typed, saying what is wrong with it.
- **Progress**: a command shows a progress toast the moment it starts, and its result takes that toast over. When the command lands, the TUI fetches the controller's data again, so the screens show the change without waiting for the next poll.
- **Checked results**: where the controller's record can be read back (a firewall rule's switch, an SSID's settings, a port's PoE mode or name, a fixed IP), the result is checked against it. A change the controller accepted but did not apply is reported as such, not as done.
- **History**: `n` opens the last 50 notifications, newest first, with the time each was shown. `Esc` or `n` closes it.

A key that cannot do anything for the selected row says why in a toast (an offline device cannot be restarted, a wired client cannot be kicked, a firmware upgrade needs a controller session this profile lacks) instead of sending a command that would fail. A jump to a device or client that has left the controller says so and stays on the screen you are on.

## Screen Reference

### Dashboard

Eight live panels:

| Panel             | What It Shows                                                                                     |
| ----------------- | ------------------------------------------------------------------------------------------------- |
| **WAN Traffic**   | Mirrored chart (download up, upload down, each on its own scale), live TX/RX rates, peak tracking |
| **Gateway**       | Model, firmware, WAN IP, IPv6, DNS, ISP name, latency, uptime, last speed test                    |
| **Connectivity**  | Subsystem status dots (WAN/WWW/WLAN/LAN/VPN), aggregate traffic bars                              |
| **Capacity**      | Color-coded CPU/MEM gauges, load averages, device/client fleet count                              |
| **Networks**      | One line per network by VLAN ID under a header: Name, VLAN, Subnet, Clients, and v6               |
| **WiFi / APs**    | Client count and WiFi experience percentage per access point                                      |
| **Top Clients**   | Proportional traffic bars with fractional block characters                                        |
| **Recent Events** | Compact event display, color-coded by severity                                                    |

The title counts the controller's active alarms. A panel with nothing to show says whether it is still connecting or which access it lacks. A WAN rate that has stopped arriving is not drawn as a flat line: the panel says how old the last one is (`no rate for 47s`), or `not connected`. The Gateway panel's DNS line lists the servers that fit whole, then `+N` for the rest. The status bar says where `Enter` goes from the focused panel (`Enter open Events`).

| Key                      | Action                                                                         |
| ------------------------ | ------------------------------------------------------------------------------ |
| `h` `j` `k` `l` / arrows | Move the focus between panels                                                  |
| `g` / `G`                | First / last panel                                                             |
| `Enter`                  | Open the focused panel's screen (the Gateway panel opens the gateway's detail) |
| `S`                      | Run a WAN speed test (asks first)                                              |

The speed test needs a gateway on the site and a direct controller session. Its result shows on the Gateway panel's `Speed` line about a minute later, as download, upload, and how long ago it ran (`↓2.4G ↑1.9G just now`).

### Devices

The table filters by type and sorts by name, status, clients, traffic, or uptime. The Updates filter carries the count of devices with firmware waiting (`Updates 1`), and the sort shows at the right of the toolbar (`s sort name ↑`). `Enter` opens a detail with a tab per aspect the device has: Overview (identity, uplink, and the CPU, memory, load, and traffic figures), Radios on access points, Clients, and Ports on anything with ports. The pane takes the rows its tab needs, and the table keeps the rest. A gateway's Clients tab lists every client that reaches it, with a Via column naming the switch or access point each one attaches to; other devices list their own. The Ports and Clients tabs have a cursor of their own, with a position such as `port 3 of 24` on the pane's border (`port 9 · 3 of 4` where the port's number and its row differ), so every row is reachable on a short terminal.

Devices waiting for adoption are listed with the adopted ones on profiles that reach the Integration API, so `A` has something to act on.

| Mode                 | Key                | Action                                                            |
| -------------------- | ------------------ | ----------------------------------------------------------------- |
| List                 | `h` / `l`          | Filter (All / Gateways / Switches / APs / Offline / Updates)      |
| List                 | `s` / `r`          | Sort by the next column / reverse the sort                        |
| List                 | `Enter`            | Open the device                                                   |
| List and detail      | `R`                | Restart the device (asks first)                                   |
| List and detail      | `L`                | Locate: flash the LED, and again to stop                          |
| List and detail      | `U`                | Upgrade firmware when an update is waiting (asks first)           |
| List and detail      | `A`                | Adopt a device that is pending adoption (asks first)              |
| List and detail      | `P`                | Provision the device again (asks first)                           |
| List and detail      | `T`                | Run a speed test on a gateway (asks first)                        |
| Detail               | `h` / `l`          | Previous / next detail tab                                        |
| Detail               | `j` / `k`, `g`/`G` | Move through the ports or clients; on other tabs, the next device |
| Detail               | `F`                | Remove the device from the site (asks first)                      |
| Detail               | `Esc`              | Close the detail                                                  |
| Detail (Clients tab) | `Enter`            | Open that client on the Clients screen                            |
| Detail (Ports tab)   | `C`                | Power cycle the selected PoE port (asks first)                    |
| Detail (Ports tab)   | `O`                | Turn the port's PoE off (asks first), or back on as it was        |
| Detail (Ports tab)   | `N`                | Rename the selected port                                          |

The Ports tab shows each port's mode, native network, and tagged networks where the controller has a Session API. The PoE and rename keys write port settings, which need that session too; without one the tab says so in place of the profile columns and the keys are not offered. The tab reads the port settings again on every device refresh, `Ctrl+r` included.

- **PoE**: `O` needs the port's settings, so while they load it asks you to press it again in a moment. Turning PoE back on restores the mode the port had when the TUI last saw it on (passive 24V stays passive 24V) without asking. When that mode was never seen this session, it asks before writing auto.
- **Remove**: `F` on an online device factory resets it, and it has to be adopted again. An offline device is only dropped from the site, and the confirmation says to reset it by hand. A device that is not adopted has nothing to remove.
- **Speed test**: the result of `T` shows on the Dashboard's Gateway panel about a minute later.
- **Access**: `U`, `P`, and `T` need a controller session, and `F` and the port power cycle need the Integration API. On a profile without the one a key needs, the key says so and sends nothing.

### Clients

The table filters by type and sorts by name, signal, traffic, or duration, and the sort shows at the right of the toolbar (`s sort name ↑`). `Enter` opens a detail with the address, network and VLAN, uplink, connection time, fixed IP, radio link for wireless clients, traffic, and guest and blocked state. The detail takes the rows its content needs, and the table keeps the rest.

| Mode            | Key       | Action                                                         |
| --------------- | --------- | -------------------------------------------------------------- |
| List            | `h` / `l` | Filter (All / Wireless / Wired / VPN / Guest / Blocked)        |
| List            | `s` / `r` | Sort by the next column / reverse the sort                     |
| List            | `Enter`   | Open the client                                                |
| List and detail | `b`       | Block the client, or unblock a blocked one (asks first)        |
| List and detail | `x`       | Disconnect a wireless client (asks first)                      |
| List and detail | `I`       | Set the fixed IP; clear the field to remove it                 |
| List and detail | `A`       | Authorize a guest for some minutes, or revoke one (asks first) |
| List and detail | `F`       | Forget the client (asks first)                                 |
| Detail          | `j` / `k` | Previous / next client                                         |
| Detail          | `h` / `l` | Change the filter, which closes the detail                     |
| Detail          | `Esc`     | Close the detail                                               |

- **Blocked** lists the connected clients that are blocked and, with a controller session, the offline ones the controller remembers. An offline row takes `b` to unblock it and `F` to forget it, and nothing else.
- **Fixed IP**: `I` works on wired and wireless clients. The network is picked from the address, and an address outside every network's subnet changes nothing and says so.
- **Access**: `I`, `A`, and `F` need a controller session. Without one they say so before asking anything.

### Networks

`Enter` shows or hides a detail panel with the network's status, purpose, zone, gateway and subnet, DHCP range, lease, DNS, and IPv6 settings. A network that relays DHCP to another server reads `Relay` in the DHCP column, and its detail names the servers.

| Mode | Key                 | Action                                   |
| ---- | ------------------- | ---------------------------------------- |
| List | `Enter` / `Esc`     | Show or hide the detail / hide it        |
| List | `e`                 | Edit the selected network                |
| List | `c`                 | Create a network                         |
| List | `D`                 | Delete the selected network (asks first) |
| Form | `Tab` / `Down`      | Next field                               |
| Form | `Shift+Tab` / `Up`  | Previous field                           |
| Form | `Space`             | Flip an on/off field                     |
| Form | `Left` / `Right`    | Move in the text                         |
| Form | `Home` / `End`      | Start / end of the text                  |
| Form | `Ctrl+w` / `Ctrl+u` | Delete a word / delete to the start      |
| Form | `Enter`             | Save                                     |
| Form | `Esc`               | Close without saving                     |

- **The form** creates and edits with the same fields: name, VLAN ID, gateway and prefix, enabled, the DHCP server with its range and DNS servers, isolation, internet access, mDNS forwarding, and IPv6. A network only offers the fields its kind can take, and one whose kind the controller does not report offers name, VLAN ID, and enabled. IPv6 shows only on a network that has it on, and can only be turned off: the controller's own interface is where IPv6 is set up. A validation error sits under the field it is about and moves the focus there.
- **Limits the form checks**: a VLAN ID from 2 to 4009 (the default network keeps VLAN 1), a prefix from /8 to /30, a DHCP range inside the subnet's usable addresses and clear of the gateway's own, and at most four DNS servers, each an IPv4 address.
- **Saving** locks the form while the controller has the draft and closes it once the new values are on screen. A save the controller refuses keeps the form open with your draft and the reason. A save that turns a network off, cuts its internet access, isolates it, or changes its VLAN, addressing, DHCP, or IPv6 lists what changes and asks first. Turning the DHCP server off deletes its range, lease time, and options, turning one on replaces a relay, and turning IPv6 off deletes its settings; the form and the confirmation both say so.
- **Deleting** asks the controller what uses the network, and the confirmation names it (`In use by 14 clients, 1 WiFi network`). If you have moved on by the time that check returns, no dialog opens: a toast says to press `D` on the network again. When the check itself fails, the confirmation says the controller refuses the delete if something uses the network. The default network cannot be deleted.

### Firewall

Four sub-tabs, each with its row count: Policies, Zones, ACL Rules, and NAT. Policies sit under a row naming their zone pair and are numbered by their place in it, the order they apply. `Enter` opens a detail panel on any row with what the table cuts: a policy's description, zones, full source and destination filters with ports, protocol, IP version, connection states, schedule, logging, and origin, and the same for ACL rules, NAT rules, and zones.

| Sub-tab           | Key                  | Action                                              |
| ----------------- | -------------------- | --------------------------------------------------- |
| All               | `h` / `l`            | Previous / next sub-tab                             |
| All               | `Enter` / `Esc`      | Show or hide the detail / hide it                   |
| Policies          | `E`                  | Enable or disable the policy (asks first)           |
| Policies          | `L`                  | Turn the policy's logging on or off                 |
| Policies          | `K` / `J`            | Pick the policy up and move it up / down            |
| Policies          | `D`                  | Delete the policy (asks first)                      |
| Policies (moving) | `K` / `J`, `k` / `j` | Move the held policy up / down within its zone pair |
| Policies (moving) | `Enter`              | Apply the move                                      |
| Policies (moving) | `Esc`                | Put the policy back                                 |
| ACL Rules, NAT    | `E`                  | Enable or disable the rule (asks first)             |
| ACL Rules, NAT    | `D`                  | Delete the rule (asks first)                        |

Reordering is pick up, move, apply. `K` or `J` picks the selected policy up and moves it one place on screen only; the row is marked while it is held, the toolbar reads `moving 1 down, not saved`, and nothing reaches the controller. Keep moving it with `K`/`J`, `k`/`j`, or the arrows, then press `Enter` to write the result as one ordering, or `Esc` to put it back. A toast confirms the move, and a move the controller refuses raises an error and leaves the table as it was. The move is written only if the controller still has the order the table showed. When it does not, nothing is written, the list is refreshed, and a warning says to move the policy again.

Rules the controller owns carry a mark and are not yours to change. A system-defined policy, a policy derived from another rule, and an ACL or NAT rule the controller manages refuse `E`, `D`, and a move on the spot, each with its reason, and the status bar and help leave those keys out for such a row. Where a key works, the status bar says what it will do for the selected row: `E enable` or `E disable`, `L log on` or `L log off`. Logging is the exception: `L` works on a system-defined policy too. A move steps over the system-defined policies of a zone pair as one block. Zones are read-only.

Every `E` and `L` is checked against the list read back from the controller. A switch the controller accepted but did not apply is an error (`The controller accepted the change, but X is still enabled`), not a success.

### Topology

The screen draws the uplink tree the controller reports: the gateway at the top, every switch and access point under the device it uplinks through, and the parent's port number on each wired link. Dashed lines are uplinks the controller reports as wireless (mesh); a link whose medium is unknown is drawn as a plain line. A device with no reported uplink hangs off the gateway on a dim dotted line, so missing uplink data shows up as such instead of being guessed. Boxes are sized to their names, and when a parent's children would not fit across the screen they wrap onto extra rows hanging off a trunk.

The map opens fitted: the largest zoom that shows the whole tree, following the terminal size. The fit never goes below 80%, the smallest zoom that keeps names whole, so a tree too tall for that starts at the gateway and runs off the bottom. The title shows the zoom, which way more of the tree lies when it does not all fit (`80% fit · more ↓`), and the selected device. The map's bottom border carries the legend (down, busy, wired, mesh, uplink unknown), as many entries as fit.

| Key                      | Action                                         |
| ------------------------ | ---------------------------------------------- |
| `h` `j` `k` `l` / arrows | Pan the map                                    |
| `PgUp` / `PgDn`          | Pan a screen up / down                         |
| `g` / `G`                | Top / bottom of the tree                       |
| `+` / `-`                | Zoom in / out, holding the center              |
| `f`                      | Fit the whole tree                             |
| `r`                      | Reset to 100%                                  |
| `[` / `]`                | Previous / next device, bringing it into view  |
| `Enter`                  | Open the selected device on the Devices screen |
| `Esc`                    | Clear the selection                            |

### Events

The feed orders events by time and drops repeat deliveries. One toolbar row heads it: the categories on the left, and on the right the feed state and the count of active alarms, each a button that shows its key (`Space ● live`, `v alarms 4`). The feed state says how events arrive: `● live` over the WebSocket, `◐ polling` when the profile has session access without a cookie login, `‖ paused`, or `○ no access`. Once older history is loaded, the right of the top border says how far back the feed reaches (`back to 11-06 17:51`, with `+ blocks` when firewall blocks are in). While the feed is paused, the right of the bottom border gives the cursor position (`event 312 of 314`). Below about 100 columns the feed state and the alarm count move to the title, and `Space pause` and `v alarms` show in the status bar.

An open event names its device and client. Where the message only spells out an address (`A network intrusion attempt from 171.25.193.9 to 10.4.20.198`), the detail adds a line for each address a known device or connected client holds, and `d` or `c` opens it.

| View        | Key         | Action                                          |
| ----------- | ----------- | ----------------------------------------------- |
| Feed        | `Space`     | Pause / resume the live feed                    |
| Feed        | `j` / `k`   | Move the cursor; moving back pauses a live feed |
| Feed        | `Enter`     | Open the selected event                         |
| Feed        | `h` / `l`   | Filter by category                              |
| Feed        | `/`         | Filter by message text                          |
| Feed        | `o`         | Load the events before the oldest one shown     |
| Feed        | `Esc`       | Cancel a history load that is running           |
| Feed        | `f`         | Bring firewall blocks in, or take them out      |
| Feed        | `p`         | Show or hide the events-by-hour heatmap         |
| Feed        | `v`         | Show the alarms                                 |
| Feed detail | `d` / `c`   | Open the event's device / client                |
| Feed detail | `Esc`       | Close the detail                                |
| Alarms      | `x`         | Archive the selected alarm (asks first)         |
| Alarms      | `X`         | Archive every alarm (asks first)                |
| Alarms      | `Enter`     | Open the alarm's device                         |
| Alarms      | `r`         | Read the alarms again                           |
| Alarms      | `v` / `Esc` | Back to the event feed                          |

- **Detail**: the whole message, type, severity, and dated time. Opening it pauses a live feed so the event stays put, and a paused feed counts what arrived below it.
- **Categories**: All, Device, Client, Network, System, Admin, Firewall, VPN, and Other.
- **Older history**: each press of `o` loads the events before the oldest one in the feed, a page at a time. A full page says `o for more`. A window with nothing in it widens by itself, from 24 hours to 3 days, 7 days, and 30 days, and the top border says so when nothing older is left (`no older events in 30d`). The feed keeps the newest 10,000 events and says when that limit is reached. `f` does nothing while a load runs.
- **Heatmap**: the legend is a `less ░▒▓█ more` scale at the right of the hour axis. `p` on a terminal too small to draw it says so.
- **Alarms**: the controller's unarchived alarms, newest first and counted in the title, read again every minute while the Dashboard or Events is in front, and after every archive. Archiving one alarm also archives the alerts the controller groups with it, and the confirmation says so. An archive the controller accepted while still listing the alarm as active is a warning, not a success. An empty view offers only `v` and `r`.

Events, older history, firewall blocks, and alarms all need a direct controller session.

### Stats

WAN bandwidth, client counts, and DPI application and category breakdowns for one of four periods: 1h, 24h, 7d, and 30d. The periods sit on the toolbar, and a reload under way or one that failed shows at its right.

| Key       | Action                            |
| --------- | --------------------------------- |
| `h` / `l` | Shorter / longer period, wrapping |
| `r`       | Reload this period and the DPI    |

Charts are only drawn under the period they belong to. While a period loads the screen says so, a failed load shows its reason in place of the charts, and a profile without session access is told statistics need one. The DPI panels cover all time and show their own read failures. Their totals are reused for two minutes when you step between periods; `r`, a click on the active period, and `Ctrl+r` always read them again.

### WiFi

Five sub-tabs: Overview, Clients, SSIDs, Neighbors, and Roaming. Each keeps its own sort, search filter, and panel, so nothing done on one changes another. Tables draw straight into the screen frame, and the access point summary line shows on Overview only. A detail or panel takes the rows its content needs: it sits beside its table on a screen at least 110 columns wide, and under the table below that.

| Sub-tab   | Key             | Action                                                                  |
| --------- | --------------- | ----------------------------------------------------------------------- |
| All       | `h` / `l`       | Previous / next sub-tab                                                 |
| Overview  | `Enter`         | Show or hide the access point's detail                                  |
| Overview  | `Space`         | Focus the tabs on this access point, and again to unfocus               |
| Overview  | `c`             | Show or hide the channel map                                            |
| Overview  | `[` / `]`       | Switch band in the channel map                                          |
| Overview  | `s`             | Cycle the sort (health, clients, channel, name)                         |
| Overview  | `o`             | Open the access point on the Devices screen                             |
| Overview  | `R` / `L` / `U` | Restart (asks first) / locate / upgrade a waiting firmware (asks first) |
| Clients   | `Enter`         | Show or hide the client's WiFi experience                               |
| Clients   | `o`             | Open the client on the Clients screen                                   |
| Clients   | `r`             | Open the client's roam history                                          |
| Clients   | `s`             | Cycle the sort (health, signal, name)                                   |
| Clients   | `b` / `x` / `F` | Block or unblock / disconnect / forget the client (each asks first)     |
| SSIDs     | `Enter`         | Show or hide the SSID's detail                                          |
| SSIDs     | `E`             | Enable or disable the SSID (asks first, both ways)                      |
| SSIDs     | `N`             | Rename the SSID (asks first)                                            |
| SSIDs     | `P`             | Change the passphrase, typed twice (asks first)                         |
| SSIDs     | `H`             | Hide the SSID, or announce its name again (asks first, both ways)       |
| SSIDs     | `D`             | Delete the SSID (asks first)                                            |
| Neighbors | `s`             | Cycle the sort (signal, channel, security, name)                        |
| Neighbors | `f` / `[` `]`   | Next band / previous and next band                                      |
| Roaming   | `r` / `Enter`   | Refresh the history                                                     |

- **AP focus** narrows the Overview, Clients, and Neighbors sub-tabs to one access point. It shows as a chip at the right of the toolbar on every sub-tab (`focus ap-atrium ✕`), dimmed where it narrows nothing, and `Esc` clears it once the sub-tab's panel is closed.
- **Band**: the band on show is a toolbar chip with its key, `f band 2.4 GHz` on Neighbors and `[/] band 2.4 GHz` while the channel map is open.
- **Channel map**: every radio of every online access point counts on its band, next to the neighbors heard there, with DFS channels marked. A neighbor heard by several of your access points counts once, by its BSSID.
- **WiFi experience** shows a client's signal, rates, and the uplink path between it and the gateway.
- **SSIDs** lists every broadcast with its state, client count, security, bands, network, and hidden flag. The controller's list leaves out visibility, isolation, band steering, fast roaming, and MLO, so the sub-tab reads each SSID in full. Until that read lands the Hidden column shows `…`, and `?` when it failed. The detail rows show `…` while reading, `unknown` when the read failed, and `─` for a setting the controller's record does not carry. A setting is never drawn as "no" before it is known.
- **SSID writes**: `H` reads the SSID from the controller first, then asks `Hide X?` or `Announce X again?` by what it found. `E` asks before enabling as well as before disabling, since either one reprovisions the access points. A second write to an SSID while one is still being applied is refused with a toast. Each write is checked against the record read back, and one the controller accepted but did not apply is reported as a failure.
- **Rename**: a name holds up to 32 bytes (32 plain characters, fewer with accents or emoji), and a name another SSID already has is refused. In a `--demo` session the prompt opens empty, so a stand-in name is never written back.
- **Passphrase**: 8 to 63 ASCII characters, asked for twice because it is typed hidden (`Ctrl+t` shows it). The current passphrase is never shown: the detail's Passphrase row reads `not shown (P to set)`, a count such as `3 per-network keys`, `RADIUS sign-in`, or `none`. `P` is refused on open and enterprise SSIDs, which have no passphrase, and on an SSID that gives each network its own key, which has no single passphrase to change.
- **Roaming** shows the history of the client selected on the Clients sub-tab, in local time, and stays up when that client leaves the list. The toolbar names the client at its right. When a refresh fails, the history already on screen stays and the toolbar adds `refresh failed`.

The access point list comes with the devices. SSIDs come from the Integration API. Wireless client data, WiFi experience, the channel map, neighbor scans, and roam history need a direct controller session, and each panel says so when the profile has none.

## Lists

Every list screen orders its rows the way the CLI does: devices by name, clients by display name, networks by name, and firewall policies grouped by source zone, then destination zone, in the order they apply. The cursor stays on the same device, client, or policy when data refreshes or the order changes, so an action key always targets the row you see highlighted.

`/` opens a search bar on the Devices, Clients, Events, and WiFi screens that filters the list as you type. `Enter` closes the bar and keeps the filter, which stays on show above the list; `/` reopens the bar on that filter to refine it, and `Esc` clears it. `Up`, `Down`, and the page keys still move the filtered list while the bar is open. The other screens (and the Alarms view and the WiFi Roaming sub-tab) have nothing to filter, so `/` there only says where search works, and their status bar and help overlay leave it out.

Columns size themselves to their content, and wider terminals reveal more of them:

| Screen               | Extra columns                                                        | From width  |
| -------------------- | -------------------------------------------------------------------- | ----------- |
| **Devices**          | Firmware (with an arrow when an update is waiting) and Clients       | 120 columns |
| **Clients**          | Network and Uplink (the access point, or the switch and port number) | 120 columns |
| **Firewall** (rules) | Zones                                                                | 96 columns  |

Narrower terminals shrink names first, ending a cut name in an ellipsis, and then drop those extra columns, so every table stays readable down to 80x24. An empty table says why: still waiting for the controller, nothing on the site, or not readable with this profile's access.

## Charts

The WAN and Stats charts draw as pixel images on terminals with a graphics protocol (kitty, Ghostty, WezTerm, iTerm2, and Sixel terminals) and as Braille or block glyphs elsewhere. The newest value sits in a rail at the chart's right edge. unifly deletes each kitty chart image once it is no longer shown and clears the rest when the TUI exits, so a long session does not fill the terminal's image memory. Set `UNIFLY_DISABLE_GRAPHICS=1` to force the glyph renderer, or `UNIFLY_GRAPHICS_PROTOCOL` to pick a protocol when detection guesses wrong.

## Data Refresh

{% mermaid() %}
graph LR
API["Integration + Session APIs"] -->|"Every 10s"| DS["DataStore"]
WS["WebSocket"] -->|"Real-time push"| DS
DS -->|"watch channels"| TUI["TUI Screens"]

    style WS fill:#50fa7b,color:#0a0a0f
    style DS fill:#80ffea,color:#0a0a0f

{% end %}

- **Devices and clients**: polled every 10 seconds from both APIs (every 60 seconds through the cloud connector)
- **Health and system info**: polled on the same cycle
- **Events**: pushed via WebSocket in real time with a cookie session, and picked up by the poll otherwise
- **Bandwidth**: sampled from device stats on each refresh cycle
- **After a command**: fetched again as soon as the command lands
- **On demand**: `Ctrl+r` refreshes now, and on the Stats screen reloads the period too
- **One at a time**: refreshes never overlap, so data fetched before a change cannot land on top of data fetched after it

## Access by Authentication Mode

The TUI works with every authentication mode. What a screen can read and change follows the APIs the profile reaches, and a screen that lacks one says which, in the panel that would have shown the data.

| What                                                                                            | API Key | Username/Password | Hybrid | Cloud   |
| ----------------------------------------------------------------------------------------------- | ------- | ----------------- | ------ | ------- |
| Dashboard, Devices, Clients, Topology                                                           | Yes     | Yes               | Yes    | Partial |
| Networks; Firewall policies, zones, and ACL rules; WiFi SSIDs (Integration API)                 | Yes     | No                | Yes    | Yes     |
| Stats, Events, Alarms, NAT rules, WiFi observability, switch port settings (controller session) | Yes     | Yes               | Yes    | No      |
| Live event stream (WebSocket)                                                                   | Polled  | Live              | Live   | No      |

- **API Key** covers both APIs on UniFi OS: the key also signs the session HTTP endpoints. Events arrive by polling, since the WebSocket needs a cookie login.
- **Username/Password** has no Integration API, so the Networks screen, the Firewall policies, zones, and ACL rules, and the WiFi SSIDs sub-tab ask for an API key.
- **Hybrid** reaches everything, with live events.
- **Cloud** routes Integration API traffic through the [Site Manager connector](/guide/cloud), one console at a time, with no controller session. Session-backed screens say so and stay empty, and client rows miss their session enrichment fields.

On a cloud profile's Devices screen, `R` restart, `L` locate, `A` adopt, `F` remove, and the port power cycle work, since they go through the Integration API the connector proxies. `U` upgrade, `P` provision, the speed test, and port profiles with their PoE and rename keys need a controller session: the Ports tab says so in place of the profile columns, and each of those keys says so and sends nothing. On the Clients screen the same goes for `I` fixed IP, `A` guest authorization, and `F` forget, and the Blocked filter lists only the blocked clients that are connected.

{% tip() %}
**API Key mode** works for most of the TUI on UniFi OS. Use **Hybrid mode** when you want events pushed live over the WebSocket instead of picked up by polling.
{% end %}

## Demo Mode

Two flags make the TUI safe to show on screen.

`--demo` (or `UNIFI_DEMO=1`, or `[demo] enabled = true` in config) connects to your real controller and swaps names, MACs, IPs, and SSIDs for deterministic fakes before anything is drawn. Switch port labels are swapped with their device, and an error toast shows `controller.example` where it would name the controller's address. The setup wizard is redacted too when it opens in such a session: every field is masked and `Ctrl+T` is refused, sites read `Site 1`, `Site 2`, and the profile name, the launch error, the controller address, and the config path are not shown.

`--demo-fleet` (or `UNIFI_DEMO_FLEET=1`) skips the controller entirely and never reads the config file, so the theme comes from `--theme` or `UNIFLY_THEME`. The TUI runs against a synthetic, offline network for a fictional creative studio, Neon Harbor Studios, and a simulation animates it about once a second:

- **The fleet**: a UDM Pro Max gateway, three switches, and nine access points (one offline, one finishing a firmware upgrade, one with an update waiting), about 140 clients on six VLANs and four SSIDs, firewall zones and policies, NAT, ACLs, alarms, and a week of event history.
- **What moves**: WAN throughput (with render-upload and asset-sync bursts), device CPU, memory, and load, client traffic and signal, roams, arrivals and departures, and security events such as IPS alerts and firewall drops.
- **Every screen has data**, including Stats history for all four periods, DPI, Wi-Fi neighbors, channel usage, roam history, and per-client Wi-Fi experience.

```bash
unifly tui --demo-fleet                  # The default fleet (seed 2077)
unifly tui --demo-fleet --demo-seed 7    # A different fleet, just as reproducible
unifly tui --demo                        # Falls back to the demo fleet when no controller is configured
```

The same seed reproduces, from launch:

- **The fleet**: every name, model, MAC, IP, ID, port, channel, SSID, and policy.
- **The animation**: WAN traffic, device telemetry, client traffic, roams, arrivals, departures, and events, tick for tick.
- **History**: the Stats charts for every period, the week of past events, roam history, and Wi-Fi neighbors, all laid out relative to the launch time.

Timestamps follow the wall clock, so the same history appears shifted to whenever you launch, and anything you trigger interactively (a restart, a kick, a block) changes what happens after it. To reshoot a sequence, relaunch with the same seed and repeat the same keystrokes at the same moments.

When `--demo` is set and no controller can be resolved from flags, environment, or config, the TUI opens the demo fleet instead of onboarding and logs that it did. The sanitizer never runs on the demo fleet, because its data is fictional already: documentation and private address ranges, locally administered MACs, and invented names.

Most of what the TUI can change plays out in the simulation, with the events and the validation a controller would give:

- **Devices and ports**: restart, locate, upgrade, provision, speed test, port power cycle, PoE off and on, and port rename. An access point can be removed, after which it waits as a pending device, and adopted again.
- **Clients**: block, unblock, kick, forget, guest authorization, and fixed IPs.
- **Networks**: create, edit, and delete, including the refusals (a VLAN in use, a DHCP server without a range).
- **Firewall**: policy enable, logging, delete, and reorder, and ACL and NAT rule toggles and deletes.
- **WiFi**: SSID enable, hide, rename, passphrase, and delete.
- **Alarms**: archive one or all.

Removing the gateway or a switch is refused with a clear error. Settings (`,`) stays closed while the demo fleet is on screen, so a recording never shows or connects a real profile.

`--demo-fleet` and `--demo-seed` are global flags, so a curated set of read-only CLI commands answers from the same fleet for terminal screenshots; see [Demo Fleet](/reference/cli#demo-fleet) in the CLI reference.

For recordings, `--no-effects` (or `NO_EFFECTS=1`) skips the launch animation.

## Setup and Settings

The setup wizard opens when the launch has no profile it can connect with: a first run, a profile named with `-p` that does not exist yet, or a saved profile that could not connect (the first step then says why). It asks how unifly signs in, the controller address and whether to verify its certificate (skipped for Cloud), and the credentials, then tests the connection. With several sites on the controller it asks which one. Nothing is saved until a test passes.

Settings (`,`) is the same form for the profile the session runs on, plus the theme and the sponsor button. `Enter` tests the connection with what the form holds; a test that passes saves the profile and reconnects with exactly what was tested. `Esc` cancels a running test. Closing the form, switching profiles, or starting a new profile with unsaved edits asks first in the usual confirmation dialog: `y` drops the edits, `n` or `Esc` keeps them, and `Enter` does not answer it.

| Key                | Action                                                           |
| ------------------ | ---------------------------------------------------------------- |
| `Tab` / `Down`     | Next field                                                       |
| `Shift+Tab` / `Up` | Previous field                                                   |
| `Left` / `Right`   | Change a picker (profile, auth mode, site); move in a field      |
| `Space`            | Toggle a checkbox                                                |
| `Ctrl+T`           | Show or hide the API key or password in the focused field        |
| `n` on Profile     | Name a new profile and set it up in the wizard                   |
| `Enter`            | Test, save, and reconnect (next step in the wizard)              |
| `r` in the wizard  | Retry a test that failed                                         |
| `t` in the wizard  | Turn TLS verification off and retry, after a certificate failure |
| `Esc`              | Cancel a test, go back a step, or close Settings                 |
| `F1`               | Help (`?` would type into the field)                             |

Text fields take the usual line-editing keys (`Home`, `End`, `Ctrl+A`, `Ctrl+E`, `Ctrl+W`, `Ctrl+U`, `Ctrl+K`) and a paste; a newline in a paste never submits the form. Both forms take every key themselves, so the global shortcuts wait until they close; `F1`, `Ctrl+c`, and `Ctrl+l` still work, and so do the dialogs the forms raise.

- **Profiles**: the Profile row walks the saved profiles. Picking another one and passing its test moves the session to it.
- **Sites**: once a test or the running session has read the controller's sites, the Site field offers them as a picker. Typing a letter switches back to a name typed by hand.
- **MFA**: the One-time code field takes a current 6-digit code for the next sign-in. The code is never saved, the field empties after every test result, and the session it opens is the one the dashboard continues with, so one code is enough. A profile's `totp_command` or `totp_env` is shown under it, the command by its program name only (`from a command (oathtool)`), never its arguments. A code given once (typed, `--totp`, or `totp_env`) is sent once per controller and never replayed: a later reconnect asks `totp_command` for a fresh one, or stops and says a two-factor code is required.
- **Secrets**: a typed API key or password is stored in the OS keyring. Without a usable keyring it falls back to plaintext in `config.toml`, and the form says so before you move on. An empty field shows where the value in use comes from (the keyring, or an environment variable), and a key or host ID field says `$NAME is used while it is set` when an environment variable still wins over what the field shows.
- **What a save writes**: only what the form changed. A profile's `api_key_env` and `host_id_env` and the credentials of the auth modes not in use are kept as they were.
- **TLS verification**: the checkbox says what ticking it does (`turns certificate checks off`), and once ticked reads `off: any certificate is accepted` in the warning color. Left alone, it follows the CA certificate field as you edit it. The wizard's last step warns when the saved profile skips verification.
- **Launch flags**: `--site`, `--insecure`, and the other connection flags keep overriding the profile after Settings reconnects, and the fields they override are marked. They stay with the profile the launch selected. Once the form changes a value a flag covers, the flag stands down: its mark goes, a notice says the test and the session after it use what is typed, and the session runs without that flag after the save. In the wizard the flags fill in the steps ahead of time instead, and what the steps show is what gets tested and saved. A `--site` given at launch is always shown on the Site step before anything is saved. `--timeout` and `--no-cache` stay with the session and are never written to the profile.
- **CA certificate and timeout** are edited here too. An empty timeout inherits `[defaults]`. A draft that could not be loaded again (a CA file that does not exist, a timeout of 0) goes back to the step with that field before any test runs.
- **Test sessions**: a sign-in a test opened is logged out when its result is discarded (the form closes, or another test replaces it). A session left behind by a unifly that exited mid-setup is cleaned up at the next launch.

The test signs in with the credentials typed in the form, not a cached session or `UNIFI_PASSWORD`. When it fails, the form explains the failure and what to do next (an unverified certificate, rejected credentials, a missing second factor, an unknown site, an unreachable address, an address that is not a UniFi controller), and moves focus to the field it is about.

## Theme

The TUI uses the [Opaline](https://crates.io/crates/opaline) theme engine with the SilkCircuit color palette. Press `,` to open Settings and use the theme selector to preview and switch themes. Theme changes take effect immediately and persist to `defaults.theme` in your config file. You can also set the initial theme via the `UNIFLY_THEME` environment variable at launch.

## Connection Resilience

When a connection attempt fails or the controller drops mid-session, the TUI reconnects automatically. Retries use exponential backoff starting at 2 seconds and doubling up to a 30-second cap, and they continue indefinitely; the status bar shows the current attempt number while reconnecting. Press `Ctrl+r` while disconnected to retry immediately instead of waiting out the backoff.

A link lost after a good connect shows too. One failed refresh is treated as a slow answer: the status stays `connected` and nothing on screen changes. The second failure in a row turns the status bar to `reconnecting`, and any refresh that succeeds (the next poll, a `Ctrl+r`, or the one after a command) turns it back to `connected`. While it reads `reconnecting` the screens keep the data they have, and a network save, a policy move, or any other command is still sent, since the controller may well answer it.

Switching to another profile from Settings shows `connecting` at once and does not wait for the controller being left to answer.

On `SIGTERM`, `SIGHUP`, `SIGINT`, `SIGQUIT`, or a closed input stream the TUI quits through its normal path and restores the terminal.

## Next Steps

- [CLI Commands](/reference/cli): the full command reference
- [Authentication](/guide/authentication): understand what each auth mode enables in the TUI
- [Troubleshooting](/troubleshooting): TUI logs, display issues, and common fixes
