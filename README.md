# My World

A tiny RimWorld-style colony sim prototype in Rust ([macroquad](https://macroquad.rs)).

## Run

```sh
# once, if you don't have Rust yet:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cargo run --release
```

## How to play

You don't control colonists directly. You mark work, and they pick it up on their own.

| Input | Action |
|---|---|
| `1` Chop | mark trees → wood |
| `2` Mine | mark rock (or your own walls) → stone |
| `3` Harvest | mark berry bushes → food |
| `4` Build wall | mark grass → wall (uses 5 stone, or 5 wood if you have no stone) |
| `5` Cancel | remove markings |
| Left mouse (drag) | paint the current tool |
| Right mouse (drag) | cancel markings |
| `Space` / `Tab` / `R` | pause / cycle speed 1x-2x-4x / restart |

Colonists get hungry and eat from the food stock. If there's no food left, they starve.
Trees and bushes slowly grow back, there's a day/night cycle, and every couple of days
a wanderer may show up and join you.
