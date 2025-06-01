import importlib
import locale
from os import getenv
from string import Template

from aria_shell.utils.logger import get_loggers

""" 
Super simple internationalization for the AriaShell

Basically a stripped-down version of python-i18n on pypi (thanks!)

catalog format:
'en': {
    'hi': 'Hello ${name} !',
    'count_flat': 'test ${count} simple',
    'count': {
        'zero': 'No items :(',
        'one': 'One item',
        'many': '${count} items'
    },
},
'it': { etc... }
"""

DBG, INF, WRN, ERR, CRI = get_loggers(__name__)

_CATALOG = {}
DEFAULT_LANG = 'en'
CURRENT_LANG = 'en'


def setup_locale():
    try:
        locale.setlocale(locale.LC_ALL, '')
        _lang_code, _ = locale.getlocale()
    except locale.Error:
        WRN('Cannot read system locale, fallback to $LANG or english')
        _lang_code = getenv('LANG') or DEFAULT_LANG

    if _lang_code and len(_lang_code) >= 2:
        INF(f'Detected system locale: {_lang_code}')
        global CURRENT_LANG
        CURRENT_LANG = _lang_code[0:2].lower()
    else:
        WRN(f'Cannot detect system locale, using default translations')


def i18n(key: str, lang: str = None, **kargs) -> str:
    """ Translate the given key """
    if lang is None:
        lang = CURRENT_LANG

    # load the locale file, if not done already
    if not lang in _CATALOG:
        mod = importlib.import_module(f'aria_shell.i18n.{lang}')
        _CATALOG[lang] = getattr(mod, '__CATALOG__')

    # search the translated key in 'lang' catalog, fallback to 'en'
    trans = _CATALOG.get(lang, {}).get(key) or \
            _CATALOG.get(DEFAULT_LANG, {}).get(key)
    if not trans:
        return key

    # handle pluralization
    count = kargs.get('count')
    if count is not None and type(trans) == dict:
        if count == 0 and 'zero' in trans:
            trans = trans['zero']
        if count == 1 and 'one' in trans:
            trans = trans['one']
        if 'many' in trans:
            trans = trans['many']

    # return translated string with placeholders applyed
    return Template(trans).safe_substitute(**kargs)
