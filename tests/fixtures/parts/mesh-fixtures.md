# Curved mesh tolerance fixtures (#162)

`mesh_sphere.step` (R60 mm) and `mesh_cylinder.step` (R50 mm, height 40 mm,
centred at the origin) were generated with build123d 0.13.0 / OpenCascade 8.0:

```sh
uv run --with build123d python - <<'PY'
from build123d import Sphere, Cylinder, export_step
export_step(Sphere(60), 'tests/fixtures/parts/mesh_sphere.step')
export_step(Cylinder(50, 40), 'tests/fixtures/parts/mesh_cylinder.step')
PY
```

These synthetic fixtures were created for this repository under its Apache-2.0
license. Exporter version/header timestamps may differ when regenerated.
