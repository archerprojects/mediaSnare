project('mediasnare',
  'rust',
  version: '0.2.0',
  meson_version: '>= 1.0',
)

gnome = import('gnome')

dependency('gtk4',               version: '>= 4.14')
dependency('libadwaita-1',       version: '>= 1.5')
dependency('gstreamer-1.0',      version: '>= 1.22')
dependency('gstreamer-video-1.0')
dependency('gstreamer-audio-1.0')

prefix     = get_option('prefix')
bindir     = prefix / get_option('bindir')
datadir    = prefix / get_option('datadir')
iconsdir   = datadir / 'icons'
appsdir    = datadir / 'applications'
schemasdir = datadir / 'glib-2.0' / 'schemas'

conf = configuration_data()
conf.set_quoted('APP_ID',     'org.archerprojects.mediaSnare')
conf.set_quoted('VERSION',    meson.project_version())
conf.set_quoted('PKGDATADIR', datadir / 'mediasnare')

configure_file(
  input:  'src/config.rs.in',
  output: 'config.rs',
  configuration: conf,
)

resources = gnome.compile_resources(
  'mediasnare',
  'data/resources/resources.gresource.xml',
  gresource_bundle: true,
  source_dir: 'data/resources',
  install: true,
  install_dir: datadir / 'mediasnare',
)

install_data(
  'data/org.archerprojects.mediaSnare.gschema.xml',
  install_dir: schemasdir,
)
meson.add_install_script('build-aux/compile-schemas.sh')

install_data(
  'data/mediasnare.desktop',
  install_dir: appsdir,
)

# SVG — scalable
install_data(
  'data/icons/mediasnare.svg',
  install_dir: iconsdir / 'hicolor' / 'scalable' / 'apps',
)

# PNG — all sizes
foreach size : ['16x16', '22x22', '24x24', '32x32', '48x48', '64x64', '128x128', '256x256']
  install_data(
    'data/icons/hicolor' / size / 'apps' / 'mediasnare.png',
    install_dir: iconsdir / 'hicolor' / size / 'apps',
  )
endforeach

meson.add_install_script('build-aux/update-icon-cache.sh')

src_root   = meson.project_source_root()
build_root = meson.project_build_root()

if get_option('buildtype') == 'release'
  cargo_options = '--release'
  rust_target   = 'release'
else
  cargo_options = ''
  rust_target   = 'debug'
endif

custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: 'mediasnare',
  console: true,
  command: [
    'bash', src_root / 'build-aux/cargo-build.sh',
    src_root,
    rust_target,
    cargo_options,
    bindir,
    '@OUTPUT@',
  ],
)

custom_target(
  'deb',
  output: 'mediasnare_@0@_amd64.deb'.format(meson.project_version()),
  command: [
    'bash', src_root / 'build-aux/build-deb.sh',
    meson.project_version(),
    build_root,
    src_root / 'dist',
  ],
  build_by_default: false,
  console: true,
)
