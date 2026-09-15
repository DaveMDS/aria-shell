#
# A simple and stupid Makefile for AriaShell, for the lazy devs
#
# Maily used as a reference for the various commands used
#



.PHONY: venv-create venv-up venv-down

venv-create:
	python -m venv .venv

venv-up:
	. .venv/bin/activate

venv-down:  # THIS DOES NOT WORK, must find another way...
	deactivate


.PHONY: deps test run build install clean

deps:  # TODO: A better method?
	python -m pip install --require-virtualenv -r requirements.txt

test:
	pytest tests/ -s -v

run:
	python aria_shell/bin/aria-shell

build:
	python -m build

install:
	python -m pip install --require-virtualenv .

clean:
	rm -rf dist/ build/ aria_shell.egg-info/


# TODO: uninstall

# .PHONY: publish
# publish:
# 	python -m twine upload --repository testpypi dist/*

