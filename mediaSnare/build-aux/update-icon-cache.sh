#!/bin/bash
gtk-update-icon-cache -f -t "${DESTDIR}${MESON_INSTALL_PREFIX}/share/icons/hicolor" 2>/dev/null || true
gtk-update-icon-cache -f -t "${DESTDIR}${MESON_INSTALL_PREFIX}/share/icons/lean-icons" 2>/dev/null || true
