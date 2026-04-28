# hc-isy

[![CI](https://github.com/homeCore-io/hc-isy/actions/workflows/ci.yml/badge.svg)](https://github.com/homeCore-io/hc-isy/actions/workflows/ci.yml) [![Release](https://github.com/homeCore-io/hc-isy/actions/workflows/release.yml/badge.svg)](https://github.com/homeCore-io/hc-isy/actions/workflows/release.yml) [![Dashboard](https://img.shields.io/badge/builds-dashboard-blue?style=flat-square)](https://homecore.io/lf-workflow-dash/)

Bridges Universal Devices ISY/IoX controllers (ISY994i, eISY, Polisy) into HomeCore via REST + WebSocket.

## Supported device types

| ISY Category | HomeCore device_type | Notes |
|---|---|---|
| Dimmers (cat 1, UOM 51) | `light` | Brightness 0-100 |
| Relays/switches (UOM 78) | `switch` | On/off |
| Contact sensors | `contact_sensor` | Open/closed |
| Motion sensors | `motion_sensor` | Motion detected |
| Water sensors | `water_sensor` | Wet/dry |
| Temperature/humidity | `sensor` | Numeric value |
| Locks (UOM 11) | `lock` | Lock/unlock |
| Garage doors (UOM 97) | `cover` | Open/close |
| FanLinc | `fan` | Speed control |
| Thermostats | `thermostat` | Heat/cool/auto/setpoints |
| ISY scenes | `scene` | Activate on/off |

## Setup

1. Copy `config/config.toml.example` to `config/config.toml`
2. Set the ISY host, port, and admin credentials
3. Add a `[[plugins]]` entry in `homecore.toml`

Requires ISY firmware 4.2.3+ for WebSocket event streaming.
