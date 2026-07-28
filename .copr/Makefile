# Copr's SCM source method invokes this as:
#   make -f .copr/Makefile srpm outdir=<dir> spec=<path>
# It fetches the tagged source tarball named in the spec and builds a
# source RPM into outdir for Copr to rebuild in each chroot.

spec ?= dist/copr/antiphon.spec
outdir ?= $(CURDIR)
sourcedir := $(CURDIR)/dist/copr

.PHONY: srpm
srpm:
	dnf -y install rpm-build rpmdevtools
	spectool -g -R --define "_sourcedir $(sourcedir)" $(spec)
	rpmbuild -bs $(spec) \
	    --define "_topdir $(CURDIR)/.rpmbuild" \
	    --define "_sourcedir $(sourcedir)" \
	    --define "_srcrpmdir $(outdir)"
