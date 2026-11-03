# COPR "make srpm" source method: builds the SRPM from this checkout.
# COPR runs: make -f .copr/Makefile srpm outdir=<dir> spec=<spec>
srpm:
	dnf -y install cargo git-core tar xz gzip rpm-build
	git config --global --add safe.directory "$(CURDIR)"
	ruwac/packaging/fedora/make-sources.sh --outdir "$(outdir)"
