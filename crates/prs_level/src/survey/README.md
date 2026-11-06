# Surveys

Heights for maps of real places, as whole decimetres, a row to a line from the north-west corner. Only the
lie of the land comes from here. Coastlines, roads, buildings and everything else on a map are made up.

Terrain: NASA SRTM 1 arc-second (public domain), courtesy of NASA and USGS, from the AWS Terrain Tiles
`skadi` copy of the tiles. No OpenStreetMap or imagery is used.

Each file is made by `scripts/survey.py` from one tile, blurred by a Gaussian to take out single-pixel noise
and the steps void filling leaves:

| File | Tile | Centre | Across | Cells | Blur |
| --- | --- | --- | --- | --- | --- |
| `muttrah.txt` | `N23E058` | 23.6200 N, 58.5760 E | 2048 m | 64 | 30 m |
| `fallujah.txt` | `N33E043` | 33.34051 N, 43.77500 E | 4096 m | 128 | 45 m |

```sh
curl -O https://elevation-tiles-prod.s3.amazonaws.com/skadi/N23/N23E058.hgt.gz && gunzip N23E058.hgt.gz
python3 -I scripts/survey.py N23E058.hgt 23 58 23.6200 58.5760 2048 64 30 crates/prs_level/src/survey/muttrah.txt
```
