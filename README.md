<!-- README template from https://github.com/dthung1602/crustasync -->


[![Contributors][contributors-shield]][contributors-url]
[![Forks][forks-shield]][forks-url]
[![Stargazers][stars-shield]][stars-url]
[![Issues][issues-shield]][issues-url]
[![MIT License][license-shield]][license-url]

<!-- PROJECT LOGO -->
<br />
<p align="center">
  <a href="https://github.com/dthung1602/crustasync">
    <img src="https://raw.githubusercontent.com/dthung1602/crustasync/master/logo.png" alt="MB" height="256">
  </a>

  <h3 align="center">Crustasync</h3>

  <p align="center">
    A directory syncing cli / lib
  </p>
</p>


## Installation

1. Create a Google cloud project and setup OAuth2 for desktop
   app ([guide](https://developers.google.com/identity/protocols/oauth2/native-app))
2. Obtain the client id and client secret
   ```shell
   # export them to env variable when building
   export GOOGLE_CLIENT_ID=123456789.apps.googleusercontent.com
   export GOOGLE_CLIENT_SECRET=super-secret
   # or for convenience, save them to .cargo/config.toml
   [env]
   GOOGLE_CLIENT_ID = "123456789.apps.googleusercontent.com"
   GOOGLE_CLIENT_SECRET = "super-secret"
   ```
3. Run `cargo build`

## Usage

```
$crustasync --help

A directory syncing cli

Usage: crustasync [OPTIONS] <SRC_DIR> <DST_DIR>

Arguments:
<SRC_DIR>  Source directory. Can be relative or absolute local path. Use prefix `gd:` to indicate a GoogleDrive directory
<DST_DIR>  Destination directory, same format as SRC_DIR

Options:
--dry-run                  
--log-level <LOG_LEVEL>        [default: info] [possible values: error, warn, info, debug]
-c, --config-dir <CONFIG_DIR>  [default: /home/henry.duong/.config/crustasync]
-h, --help                     Print help
-V, --version                  Print version
```


## License

Distributed under the MIT License. See `LICENSE` for more information.


<!-- MARKDOWN LINKS & IMAGES -->
<!-- https://www.markdownguide.org/basic-syntax/#reference-style-links -->
[contributors-shield]: https://img.shields.io/github/contributors/dthung1602/crustasync.svg?style=flat-square
[contributors-url]: https://github.com/dthung1602/crustasync/graphs/contributors
[forks-shield]: https://img.shields.io/github/forks/dthung1602/crustasync.svg?style=flat-square
[forks-url]: https://github.com/dthung1602/crustasync/network/members
[stars-shield]: https://img.shields.io/github/stars/dthung1602/crustasync.svg?style=flat-square
[stars-url]: https://github.com/dthung1602/crustasync/stargazers
[issues-shield]: https://img.shields.io/github/issues/dthung1602/crustasync.svg?style=flat-square
[issues-url]: https://github.com/dthung1602/crustasync/issues
[license-shield]: https://img.shields.io/github/license/dthung1602/crustasync.svg?style=flat-square
[license-url]: https://github.com/dthung1602/crustasync/blob/master/LICENSE
