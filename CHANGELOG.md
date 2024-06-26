## [1.2.2](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.2.1...v1.2.2) (2024-06-02)


### Bug Fixes

* 🐛 add missing method to list shares ([adced10](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/adced10314a56a2e3c07eaa610ee37caade46c65))
* 🐛 concat shares for not filled backup ([f9270ba](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/f9270ba42bb4d43d49094550717b60aae1bef491))

## [1.2.1](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.2.0...v1.2.1) (2024-05-24)


### Bug Fixes

* 🐛 fix missing share when multiple share overlap each others ([b6d0ab6](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/b6d0ab6ed3581e07257b5de4db5506155a34eaf0))

# [1.2.0](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.1.2...v1.2.0) (2024-05-11)


### Bug Fixes

* 🐛 fix error message when the file can't be read ([31c6fb1](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/31c6fb198bc34d809218fd9de8b1eb5c1909c6d9))
* 🚑️ fix the fuse driver when the file is empty ([a9682dc](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/a9682dca2f144e49f49240d665ba412325e46361))
* fix date in the the fuse driver ([13aa2be](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/13aa2bef18e61c59ebf860941e645711decaf7d1))


### Features

* ✨ add attribute file reader ([da15ce5](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/da15ce538125bf937c3f1fee4ff544468b299e2e))
* ✨ add the management of hardlink on backuppc ([ad15374](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/ad15374db9e6ccc9a7da9a5ef6e02ed6405682a6))

## [1.1.2](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.1.1...v1.1.2) (2024-05-10)


### Bug Fixes

* 🧵 add info on multithread read file ([86c2126](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/86c21260672f72f5585c55fb81fc3260dd25443c))

## [1.1.1](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.1.0...v1.1.1) (2024-05-10)


### Bug Fixes

* add info to tell that trait are Send and Sync ([b2734f3](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/b2734f3ad259cff421e1f3e33e67a671506debd0))

# [1.1.0](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.0.2...v1.1.0) (2024-05-10)


### Features

* ⚡️ add a cache layer in the view ([2b23b4f](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/2b23b4fd47c46b5ccdf07958f36f7741c59ac031))

## [1.0.2](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.0.1...v1.0.2) (2024-05-08)


### Bug Fixes

* 🐛 missing view that no depend on fuse ([a7c1c70](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/a7c1c704ce156e9e1a208909df4e1d23a4ca1503))

## [1.0.1](https://gogs.shadoware.org/phoenix/backuppc_pool/compare/v1.0.0...v1.0.1) (2024-05-07)


### Bug Fixes

* ➖ remove optional dependency for the library part ([46f468b](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/46f468b3b0ac11859da18c34b3163832030de156))

# 1.0.0 (2024-05-06)


### Bug Fixes

* 🐛 better usage of clap ([5c1c40b](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/5c1c40b821c1392f01cc2bcf2681b1dd4fc24a6a))
* add missing binary + update release workflow ([a8b0cb9](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/a8b0cb956c8c0dc5cc8897ef827f36144841d3ce))
* remove some clone ([9e53f8e](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/9e53f8e7e6190b3a48dd444cf86c30f66d3636d0))
* some fix with clippy ([591c4cf](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/591c4cf403db4027524fe11a95f75b081de3c647))


### Features

* add sementic release and gitea actions ([001c246](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/001c246552228cdfa4d79e8dfcb4a5f6aabf28c0))
* add view merge ([c7cc389](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/c7cc389c09221db0cc1dfc8a271c01afd7737e60))
* **clippy:** correction venant de clippy ([c8b2e03](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/c8b2e03a14a212bfa3215991c9374b76a7df3ac0))
* **clippy:** replace *_or instead of _or_else ([473ef61](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/473ef61058668eef0688a3bb22ecbdf4f390953e))
* create a library ([1bdd7a1](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/1bdd7a19e2a1122231b33d48caf460c0c22f1675))
* create an application that can read a backuppc v4 pool ([ad665d0](https://gogs.shadoware.org/phoenix/backuppc_pool/commit/ad665d044e0dba8171b40a7f42b167048b1cc3ee))
