This directory contains files that are needed to build the compute
images, or included in the compute images.

compute-node.Dockerfile
	To build the compute image

vm-image-spec.yaml
	Instructions for vm-builder, to turn the compute-node image into
	corresponding vm-compute-node image.

etc/
	Configuration files included in /etc in the compute image

patches/
	Some extensions need to be patched to work with Neon. This
	directory contains such patches. They are applied to the extension
	sources in compute-node.Dockerfile

In addition to these, postgres itself, the neon postgres extension,
and compute_ctl are built and copied into the compute image by
compute-node.Dockerfile.

## DSGA release SLO evidence

```sh
python3 ci/dsga_release.py verify-slo --distribution distribution-manifest.json \
  --policy acceptance-policy.json --evidence acceptance-observations.json
```

The policy scope identifies the release, profile and workload. It supplies the
calibrated thresholds, minimum sample count for each metric and required hard
assertions. The observation scope uses the same identity and the SHA256 of the
exact distribution manifest. Observations carry the measured `metrics`, their
`samples` counts and the actual `hard_assertions` as boolean facts.

Durations use milliseconds. Ratios use fractions, with relative changes computed
as measured over baseline minus one. Keep the calibration status `NOT_MEASURED`
until the policy thresholds and sample requirements have been agreed. A passing
receipt covers the SLO portion of acceptance for that scope.
