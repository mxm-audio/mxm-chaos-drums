# mxm-chaos-drums — UI and catalogue brief

Required by mxm-kit's `docs/MXM_DESIGN_SYSTEM.md` §14 and written before any DSP or editor code. The
product's plan is the owner's and private; this brief is the part of it this repository holds.
**Approved by the owner, 2026-11-02** ("Approved, go ahead and start building"), so P1 starts.

**Plugin:** `mxm-chaos-drums`; permanent CLAP id `dk.mxm.mxm-chaos-drums`. An original instrument: it
copies no machine. MIDI in; stereo out, plus sixteen individual outputs; no audio input (the owner,
2026-11-02). Sixteen cells on notes 36–51, and their touch keys on notes 52–67.

**Provisional values.** Ranges, curves and defaults marked *(prov.)* are this brief's starting
points. P1's listening and P2's measurements settle them before P3 writes the parameters; the IDs,
kinds, the model domain and the catalogue's IDs do not move.

---

## How it works, for the person playing it

- **A note supplies energy; it does not fire a hit.** Each cell holds a charge with a ceiling. A
  note-on adds a **Strike** packet scaled by velocity, so a short DAW trigger still plays. Holding
  the note keeps pouring charge in (**Supply**). Sounding spends charge (**Hunger**), and **Leak**
  drains it at rest. A cell's sound can never be louder than its charge allows: an empty cell is
  silent, and a full one may sustain itself.
- **So the playing changes the sound.** Loud dies sooner, a long note sustains, fast repeats stack
  charge, and a starved cell sputters.
- **A cell plays a Model**: a drum made of up to four engines on the cell's one charge. The five
  engines change character as charge fills and drains:

| Engine | Low charge → high charge |
|---|---|
| Ring | a damped thud → a self-sustaining drone whose pitch drifts with the charge |
| Spark | silence → isolated ticks → crackle, off the beat |
| Fold | a locked tone → stepped tones → chaos and noise |
| Loop | a pluck or metal → a howl held by its own loss |
| Dust | sparse grit → dense hiss |

- **Cells are neighbours.** The sixteen sit on a 4×4 grid whose edges wrap round, so each has four
  neighbours. **Flow** moves charge from fuller cells to emptier ones; **Coupling** feeds each cell's
  sound into its neighbours. Which cells sit next to which is part of the kit.
- **They share one supply.** Every held note draws from one **Pool**, so holding many notes makes the
  whole kit sag and pump. **Bus A** and **Bus B** power the cells set to them with no note at all: a
  steady level drones, and a moving one feeds and starves them.
- **Touch keys** put a hand on a cell: holding one feeds the cell's sound back into its charge,
  bridges it to its neighbours and adds a faint skin-like noise and hum. Velocity sets how firm the
  touch is, and channel pressure scales every touch.
- **Temper** is the one dial from steady to wild. At Temper 0 there is no Flow or Coupling and no
  jitter, and each cell is a predictable drum. Raising it moves every engine toward the edge where
  small differences grow, opens Flow and Coupling, and adds seeded jitter.
- **Wild, yet exactly reproducible.** The same notes from the same start render the same samples,
  at any buffer size and on every platform, so a bounce is what was heard. **Seed** chooses the
  jitter. **Reset on play** (on by default) restarts every cell from rest when the transport starts.

## 1. Primary sound-design task

**Play a kit that breathes.** The primary gestures are choosing a cell on the grid and its model,
setting how a note powers it and how long its charge lasts, and turning Temper to choose how much
the kit runs itself. The editor leads with the grid and the kit-wide dials, then the selected cell.
It does not lead with a step row: sequencing belongs to the host.

## 2. Controls reached for most

1. **Temper**, kit-wide: steady to unpredictable.
2. **Model**, per cell: which drum it is.
3. **Strike** and **Supply**, per cell: how much a hit gives and how much a held note keeps giving.
4. **Leak** and **Hunger**, per cell: how long the charge lasts.
5. **Coarse**, per cell: the cell's pitch, so three toms are `tm1`–`tm3`.
6. **Flow**, **Coupling** and the buses, kit-wide: how much the cells feed each other and what powers
   them without notes.

## 3. Signal flow that must be visible

```text
notes 36–51 ─► cell 1…16: Strike + Supply ─► charge ─► the model's engines ─► level / pan ─┐
notes 52–67 ─► touch on cell 1…16 ──────────────▲                                        ├─► L+R ─► Master
Pool ─► every held note and every bus ──────────┤                                        └─► own outputs 1…16
Bus A / Bus B ─► the cells set to them ─────────┤
neighbours on the 4×4 grid: Flow (charge) and Coupling (sound) ◄──► each cell
LFO 1–3 / Wheel / Pressure / Velocity / Random ─► each cell's four routes
LFO 1–3 / Wheel / Pressure ─► the kit's four routes (Temper, Flow, Coupling, Pool, Recharge, buses)
```

Relationships to make plain without a manual:

- **A note is energy.** The grid shows each cell's charge rising with a hit, held up by a held note,
  and falling as it sounds.
- **Neighbours are the grid.** Flow and Coupling act only across the grid's edges, which wrap round.
- **One supply.** The Pool's level is visible and drops as notes are held.
- **Temper 0 is steady.** At zero the grid shows no Flow at all.
- **A choke latches.** A choked cell stays empty, whatever its neighbours do, until it is struck or
  touched again (see Choke group).

## 4. Play view

There is no authored Play tab. Derived pages open on Performance:

- **`lattice`**, the 4×4 grid, in grid order (cell 1 top left, rows of four). Each square shows the
  two-digit cell number, the made-up name (`bd1`), the model's label, its charge as a fill rising from
  the bottom, and compact `M`/`S` buttons. Clicking a square, or OPEN on it, selects the cell for the
  cards below and strikes it at velocity 100 of 127, as the drum machine's slot rows do. Selection is
  a fill, an outline and a rail together, never hue alone, and never moves or resizes anything.
- **`kit`**, the kit-wide dials: Temper, Flow, Coupling, Pool, Recharge, Bus A, Bus B, Seed, Reset on
  play and Dynamics, each with its route rows.

The app bar carries presets, Master, the output meter, and Resample and Export (design system §3.1).
The host or MXM Player supplies notes and touch.

## 5. Advanced controls and disclosure

- **Nothing reproduced:** there is no source panel to keep visible.
- **A model shows only the controls it reads**, under its own names, as the drum machine's do; a
  control it never reads has no knob, and its stored value stays unread.
- **Model notes**, in the Model card, are the one disclosure: what the drum is made of and how it
  answers charge, in the player's words.
- **Routes are parameter-local**, as the drum machine's: each continuous control's route rows sit
  below it, with a `‹ modulate ›` line that adds a source to a free route. The cell's four routes edit
  the selected cell; the kit's four routes sit below the kit dials.
- No randomiser, no model editor, no setup drawer.

## 6. Categories, cards and grouping

| Category | Cards |
|---|---|
| Performance | `lattice`; `kit`. Preferred together. |
| Modulators | `lfos`: three kit-wide LFO rows, each Rate, Sync and Shape, as the drum machine's. |
| Generators | `model`: the selector, Coarse, Fine and Model notes. `charge`: Strike, Supply, Hunger, Leak, Threshold. `feeds`: Bus, Link, Touch, what else powers the cell. `controls`: the model's general controls, as it names them. Each with its route rows. |
| Tone | `output`, last: Level and Pan with their route rows, Output, Choke group, and the kit-wide **All cells** row (`L+R` or `Own outputs`), as the drum machine's. |

Inside a category, the order above is the signal path. There are no Sequencers or Effects cards.
Selecting a cell changes which parameters the cards bind, never their identity or order.

## 7. Identity accent

**The collection's default teal**, dark `#4CC9D8`, light `#247F91`: no raw colour and no new token,
as the two drum instruments use. The other instruments' hues (copper, orchid, acid lime, green,
rose) and the fixed status and modulation colours already take the distinct hues, and this
instrument's identity is its grid, not a colour. Contrast, WCAG 2.x, measured 2026-11-02:

| Theme | canvas | surface-1 | surface-2 | surface-3 |
|---|---|---|---|---|
| Dark `#4CC9D8` | 9.51 | 8.84 | 8.09 | 7.11 |
| Light `#247F91` | 4.18 | 4.65 | 3.89 | 3.54 |

Every boundary passes 3:1. Accent **text** passes 4.5:1 on every dark surface, but in the light
theme only on surface-1 (the cards), so light accent text appears only there. The owner may choose
an identity hue instead; it would need its own row here.

## 8. Live visualisations

1. **Charge on the grid.** Each square's fill is that cell's charge against its ceiling; empty at
   rest.
2. **Flow on the grid's edges.** A mark on each edge whose weight is the charge that moved across it
   in the last block, and its direction. None at Temper 0. It shows the coupling actually happening,
   never decoration.
3. **The Pool and the buses.** The Pool's remaining level beside its dial; each bus's live value on
   its own dial.
4. **Cell activity.** A bounded output mark in each square; sounding, selected, and both, differ by
   fill, border and mark, not hue alone.
5. **The model's one-shot.** A compact trace of what Resample would capture: the selected cell struck
   from rest at velocity 100 with a fixed gate, its neighbours at rest. It is cached by a parameter
   signature, rendered off the audio thread, and labelled as one hit from rest, because live playing
   depends on history and this cannot show it.

Telemetry is atomics or bounded lock-free state written once per block: no history buffer, no lock
or allocation on the audio thread. Closing the editor stops it.

## 9. What is removed, and why

There is no source hardware, so nothing is translated from a panel. What it deliberately does not
have:

| Not here | Why |
|---|---|
| A step sequencer, patterns, tempo | The host and MXM Player sequence (collection rules: *does not carry what the player or the DAW already does*); Spark engines make off-grid rhythm themselves. |
| An audio input | The owner, 2026-11-02: MIDI in, audio out. |
| Built-in effects | None in the first version. |
| Per-note pressure and MPE | The collection drops per-note expression except pitch: a pitch curve on a cell key bends that cell; on a touch key it is dropped. |
| A randomise button | Unpredictability comes from Temper, coupling and history, with Seed choosing the jitter, and stays reproducible. |
| Any machine's name, panel or colours | An original. Labels and kit names are original words. |

## 10. Quarter-4K fit, minimum size and 200% zoom

As the drum machine: the opening size is derived and measured with Model notes open, under the
1920 × 1080 physical budget. The `lattice` card's floor is four squares across at the pointer floor,
each holding the widest made-up name, model label and `M`/`S` row, measured from the real controls.
The minimum holds the widest card plus gutters, held by a test like the drum machine's
`the_widest_card_fits_the_pager_viewport_at_the_advertised_minimum`. At 200% the physical window
stays fixed and paging, reflow and card scrolling keep every control reachable. Verified in P5 in
both themes at 1× and 2×; the DPI scale and window chrome are recorded then.

---

# Product surfaces

The IDs below are what P3 writes. As in every product, they may still change during pre-alpha and
freeze at the first release (the owner, 2026-10-07). **The model IDs in Appendix A are append-only
from now.** nice-plug's nested arrays make a cell's IDs `<suffix>_1` … `<suffix>_16`.

## Keys, cells and the grid

- **Cell keys** 36–51 strike cells 1–16; **touch keys** 52–67 touch cells 1–16. Note names are the
  cells' made-up names (`bd1`) and, for touch keys, `bd1 touch`. Every MIDI channel plays the kit.
- **The grid** is row-major: cell *n* sits at row ⌊(n−1)/4⌋, column (n−1) mod 4, and its neighbours
  are the cells above, below, left and right, wrapping at the edges. Cell 1's neighbours are 2, 4, 5
  and 13.
- **Made-up names** number each kind in cell order over the models alone (`bd1`, `tm1`, `tm2`), as
  the drum machine's `made_up_names`; an unavailable ID makes none and takes no number.

## Kinds

The drum machine's twenty, with their codes, so a cell's name reads the same in both instruments,
and one raw kind per engine:

`bd` kick · `sn` snare · `ch` closed hat · `oh` open hat · `hh` hat · `tm` tom · `cg` conga · `bg` bongo
· `rs` rim · `cl` clave · `cp` clap · `cb` cowbell · `cy` cymbal · `cr` crash · `rd` ride · `ma` maraca
· `tb` tambourine · `gu` guiro · `be` bell · `br` brush · **raw:** `ri` ring · `sp` spark · `fo` fold ·
`lo` loop · `du` dust.

Every model has the kind its label's last word names, held by a test.

## Model selector

- Plain domain **0…255**, fixed permanently, so the catalogue grows without moving saved values.
  Model IDs start at 1. ID 0 and every unassigned or not-yet-rendering ID are silent, shown as
  `Unavailable <id>` and never offered; the cell's Mute owns intentional silence.
- A new model takes the next unused ID. An ID never moves or changes meaning; group headings are
  display only.
- Changing Model live keeps the cell's charge; the new model's engines start from rest with it, and
  a short de-click bridges the jump. Model is automatable but not a modulation target.

## Per-cell parameters

| Suffix | Host name in cell N | Range and default |
|---|---|---|
| `model` | `Cell N model` | 0…255 stepped; Init's kit below |
| `coarse` | `Cell N coarse` | −48…+48 semitones, stepped, from a subsonic thump to a piercing tone; 0 |
| `fine` | `Cell N fine` | −100…+100 cents; 0 |
| `c01` … `c12` | `Cell N tone` … `Cell N damping`, `Cell N control 10` … `12`: the general names (below) | bipolar; zero is the model's own setting, and either end reaches two and a half times the drum's own span, out past where it stops being that drum (the owner, 2026-11-03) |
| `strike` | `Cell N strike` | the packet at full velocity, 0–100% of the ceiling; 60% *(prov.)* |
| `supply` | `Cell N supply` | how fast a held note fills the cell at full velocity, 0–100%; 30% *(prov.)* |
| `hunger` | `Cell N hunger` | how fast sounding spends charge, 0–100%; 50% *(prov.)* |
| `leak` | `Cell N leak` | the time to drain at rest, 20 ms – 30 s *(prov.)*, never infinite, so every cell comes to rest; the model's default |
| `threshold` | `Cell N threshold` | bipolar, around the model's operating point, far enough either way to tip any model into sustaining itself or breaking up at Temper 0; 0 |
| `link` | `Cell N link` | the cell's share in Flow and Coupling, 0–100%; 100% |
| `touch` | `Cell N touch` | how strongly its touch key acts, 0–100%; 50% *(prov.)* |
| `bus` | `Cell N bus` | `Off`, `A`, `B`; Off |
| `level` | `Cell N level` | gain shown in dB; Init balances the kit |
| `pan` | `Cell N pan` | bipolar; centre |
| `mute`, `solo` | `Cell N mute`, `Cell N solo` | after the cell; Mute wins over Solo, and any Solo silences the unmuted non-soloed cells |
| `choke_group` | `Cell N choke group` | `Off`, `1`…`16`; Off. A kit setting, so presets carry it |
| `output` | `Cell N output` | `L+R`, `1`…`16`; an instance setting presets never carry |

**Choke group.** A strike chokes the other cells of its group, never one struck at the same sample.
A choked cell, by its group or by the host, **latches**: it takes no Flow, Coupling or bus and stays
empty until its own next strike or touch, or a reset. A panic (CC120) silences the kit at once and
latches every cell the same way, so a bus cannot bring a drone straight back.

**The general controls.** A model reads some of these and shows only those, under its own names
where the general one would mislead. Shared meanings keep their number on every model:

| Control | General name | Meaning |
|---|---|---|
| 1 | Tone | the spectral balance |
| 2 | Body | the balance between the model's engines (a snare's body against its wires) |
| 3 | Drop | how far the pitch falls as the charge drains |
| 4 | Spread | the inharmonic ratios between partials (hats, bells, cymbals) |
| 5 | Grain | the density or rate of Spark and Dust events |
| 6 | Snap | the attack: a click, a slap, a burst |
| 7 | Drive | how hard the engines are pushed into their nonlinearity |
| 8 | Size | a Loop's length, the size of the metal |
| 9 | Damping | how fast a resonance dies, apart from the charge |
| 10–12 | — | the model's own, named by it; an exact no-op on every model that does not read them |

## Kit parameters

| ID | Range and default |
|---|---|
| `temper` | 0–100%; 0 |
| `flow` | how fast charge moves between neighbours, 0–100%, scaled by Temper; at full a full cell empties into its neighbours within a beat; 50% *(prov.)* |
| `coupling` | how much of each cell's sound reaches its neighbours, 0–100%, scaled by Temper; at full enough to drive a quiet cell into its own regime; 50% *(prov.)* |
| `pool` | the shared supply's capacity, 0–100%; 100% *(prov.)* |
| `recharge` | the time the Pool takes to refill; 500 ms *(prov.)* |
| `bus_a`, `bus_b` | each bus's level, 0–100%; 0 |
| `seed` | 1…9999 stepped; 1 |
| `reset_on_play` | on or off; on (the owner, 2026-11-02) |
| `dynamics` | the velocity curve, bipolar; 0 is linear |
| `master` | gain shown in dB |
| `lfo1_rate`, `lfo1_shape`, `lfo1_sync`, and `lfo2_*`, `lfo3_*` | as the drum machine's: 0.05–20 Hz, the collection's sync ladder; LFO 1 Sine, 2 Triangle, 3 Sine |
| `resample` | an instance setting presets never carry (mxm-drum-shell's) |

## Routes

Four routes a cell, and four for the kit, each three IDs:

```text
route<r>_source_<n>   Off, LFO 1, LFO 2, LFO 3, Wheel, Pressure, Velocity, Random
route<r>_target_<n>   Off, Pitch, Control 1 … Control 12, Strike, Supply, Hunger, Leak,
                      Threshold, Link, Touch, Level, Pan
route<r>_amount_<n>   bipolar −1…+1

kroute<r>_source      Off, LFO 1, LFO 2, LFO 3, Wheel, Pressure
kroute<r>_target      Off, Temper, Flow, Coupling, Pool, Recharge, Bus A, Bus B
kroute<r>_amount      bipolar −1…+1
```

with `r` 1…4. As the drum machine's: a route is in use when neither source nor target is Off; routes
on one source and target add, and the target's own range clamps once. A full Pitch route reaches 12
semitones; every other full route reaches its target's whole range. Random draws from the Seed's
stream, never from the system. Velocity and per-note pitch also reach the cell directly. All routes
are Off in Init.

**The count:** 41 parameters a cell (656), and 33 for the kit: **689**.

## Init

A playable kit at Temper 0, on the drum machine's role map, so the same beat plays both instruments.
Flow and Coupling sit at their defaults but do nothing until Temper rises. No bus is on.

| Cell | Key | Model | Name | Coarse | Choke |
|---|---|---|---|---|---|
| 1 | 36 | Draining kick | `bd1` | 0 | Off |
| 2 | 37 | Wired snare | `sn1` | 0 | Off |
| 3 | 38 | Falling tom | `tm1` | −5 | Off |
| 4 | 39 | Falling tom | `tm2` | 0 | Off |
| 5 | 40 | Falling tom | `tm3` | +5 | Off |
| 6 | 41 | Slapped conga | `cg1` | 0 | Off |
| 7 | 42 | Slapped conga | `cg2` | +5 | Off |
| 8 | 43 | Slapped bongo | `bg1` | 0 | Off |
| 9 | 44 | Damped rim | `rs1` | 0 | Off |
| 10 | 45 | Spark clap | `cp1` | 0 | Off |
| 11 | 46 | Leaking closed hat | `ch1` | 0 | 1 |
| 12 | 47 | Leaking open hat | `oh1` | 0 | 1 |
| 13 | 48 | Loop crash | `cr1` | 0 | Off |
| 14 | 49 | Coupled cowbell | `cb1` | 0 | Off |
| 15 | 50 | Damped clave | `cl1` | 0 | Off |
| 16 | 51 | Grain maraca | `ma1` | 0 | Off |

The closed hat cuts the open hat in Init, as a kit does. **Fifty factory kits** follow Init, installed
files on its role map (the owner, 2026-11-03, ahead of P4): five each of steady, breathing, tuned,
metal, noise, drones, feedback, starved, moving and wild, every closed hat cutting its open one.
The drone kits are *Drone*, the wild ones *Fx*, the rest *Percussion*.

**Levels balance the kit** (the owner, 2026-11-03): every cell equally loud per hit by ITU-R
BS.1770's K-weighting, then the noise cells (the hats and the maraca) and any cell a bus drones
6 dB below that, so they sit under the kick and snare. The kick is at 0 dB unless a quiet cell
would need more than the Level's +6 dB; then every Level comes down together, by up to 6 dB, so it
fits (Init's bongo loop takes it 3 dB down). The plugin's `kits` measures it, and also prints a
pink-noise balance (equal peak power per third-octave band), which no kit takes: it raises every
high cell to the Level's ceiling. Every factory kit is balanced the same way.

---

# Appendix A — the catalogue

Each model's engines are a starting point, settled by listening when it is built: P1 builds the
kick, snare and clap; P2 the raw models; P4 the rest, a group at a time. An ID is reserved now and
becomes available when its model renders.

| ID | Label | Kind | Engines |
|---|---|---|---|
| 1 | Draining kick | `bd` | a Ring whose pitch falls as it drains; a Fold click at high charge |
| 2 | Wired snare | `sn` | a Ring body spilling charge into Dust wires |
| 3 | Leaking closed hat | `ch` | Fold phases at inharmonic ratios through Dust; a fast leak |
| 4 | Leaking open hat | `oh` | the same, with a slow leak |
| 5 | Folded hat | `hh` | Fold phases tipping into noise as charge rises |
| 6 | Falling tom | `tm` | a Ring whose pitch falls as it drains |
| 7 | Slapped conga | `cg` | a Ring with a Fold slap |
| 8 | Slapped bongo | `bg` | a smaller Ring with a Fold slap |
| 9 | Damped rim | `rs` | a short, damped Ring with a Spark click |
| 10 | Damped clave | `cl` | a short, damped Ring |
| 11 | Spark clap | `cp` | Spark bursts gating Dust |
| 12 | Coupled cowbell | `cb` | two coupled Rings at an inharmonic ratio |
| 13 | Loop cymbal | `cy` | Loop metal excited by Dust |
| 14 | Loop crash | `cr` | Loop metal excited by a dense Dust burst |
| 15 | Ringing ride | `rd` | Loop metal with a bell of two Rings |
| 16 | Grain maraca | `ma` | Spark-timed Dust grains (a particle model after Cook, 1997) |
| 17 | Jingle tambourine | `tb` | Spark-timed grains striking Loop jingles |
| 18 | Scraping guiro | `gu` | a Spark train scraping a Loop, its rate set by charge |
| 19 | Coupled bell | `be` | three coupled Rings at inharmonic ratios |
| 20 | Swept brush | `br` | Dust fed by Supply, so the note's length is the stroke |
| 21 | Raw ring | `ri` | Ring alone |
| 22 | Raw spark | `sp` | Spark alone |
| 23 | Raw fold | `fo` | Fold alone |
| 24 | Raw loop | `lo` | Loop alone |
| 25 | Raw dust | `du` | Dust alone |

A model's engines share its cell's one charge, and their mix weights sum to at most one, so every
model keeps the charge law: no charge, no sound.
