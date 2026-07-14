# Changelog

All notable changes to Assura are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/).

## [0.4.0](https://github.com/assura-lang/assura/compare/v0.3.0...v0.4.0) (2026-07-14)


### Features

* assura fmt accepts directories of .assura files ([#937](https://github.com/assura-lang/assura/issues/937)) ([eb50ef9](https://github.com/assura-lang/assura/commit/eb50ef97a33ae1fc1e037baf7a01e848fcbada66))
* bound NonZeroU128 and u128/i128 for check-rust ([#1327](https://github.com/assura-lang/assura/issues/1327)) ([14a3bb6](https://github.com/assura-lang/assura/commit/14a3bb60a14c76424ec2c787c92c5649a4c570b3))
* check-rust div_ceil with NonZero divisors ([#1201](https://github.com/assura-lang/assura/issues/1201)) ([e4502bf](https://github.com/assura-lang/assura/commit/e4502bf639ee8bf00f211053485700c811339aab))
* check-rust encodes &&/||, is_multiple_of, into, as ([#1020](https://github.com/assura-lang/assura/issues/1020)) ([d9d9cc7](https://github.com/assura-lang/assura/commit/d9d9cc7b1ddc4a9ba6a313e1c84dc6d0cb24cfb5))
* check-rust encodes abs_diff, saturating_neg, ref/deref ([#1021](https://github.com/assura-lang/assura/issues/1021)) ([fc17992](https://github.com/assura-lang/assura/commit/fc1799210a26662bd21df729bb9718545fd6253d))
* check-rust encodes abs/min/max method bodies ([#985](https://github.com/assura-lang/assura/issues/985)) ([d922618](https://github.com/assura-lang/assura/commit/d922618faf57cdf63bb88da3d7736f2b43c9eeb0))
* check-rust encodes as_ref/as_mut and bool.not() ([#1023](https://github.com/assura-lang/assura/issues/1023)) ([fa740f4](https://github.com/assura-lang/assura/commit/fa740f435c079ce74b971d2a18047184f3899cba))
* check-rust encodes borrow/borrow_mut; CE for wrong is_multiple_of ([#1035](https://github.com/assura-lang/assura/issues/1035)) ([c231cd9](https://github.com/assura-lang/assura/commit/c231cd9e055c39edf565e3c5864ba3549fc3c3ef))
* check-rust encodes clone/to_owned as identity ([#1017](https://github.com/assura-lang/assura/issues/1017)) ([2c344cc](https://github.com/assura-lang/assura/commit/2c344cc739dad8b02d51ecd7801b607955092f32))
* check-rust encodes default(), MIN/MAX, and small pow ([#1022](https://github.com/assura-lang/assura/issues/1022)) ([d5ff398](https://github.com/assura-lang/assura/commit/d5ff3989cc80e4d4524afaff3c67b8ab69bb9379))
* check-rust encodes deref/deref_mut as identity ([#1036](https://github.com/assura-lang/assura/issues/1036)) ([d3ab14f](https://github.com/assura-lang/assura/commit/d3ab14ffef93713a56a508896f0925411086985c))
* check-rust encodes i64::clamp as min/max IR ([#1004](https://github.com/assura-lang/assura/issues/1004)) ([33bc150](https://github.com/assura-lang/assura/commit/33bc150dc94532b20e6da8025fe55337eb838ffe))
* check-rust encodes i64::max/min and From ([#1019](https://github.com/assura-lang/assura/issues/1019)) ([8bc0497](https://github.com/assura-lang/assura/commit/8bc04970b5746fb266bef6c2ce805e5da481d454))
* check-rust encodes i64::signum via nested if ([#1018](https://github.com/assura-lang/assura/issues/1018)) ([0aa5627](https://github.com/assura-lang/assura/commit/0aa562744dd92832c2696c7fb7d4135f0f987199))
* check-rust encodes is_positive/is_negative ([#1014](https://github.com/assura-lang/assura/issues/1014)) ([5fd7fe3](https://github.com/assura-lang/assura/commit/5fd7fe3450511ff3895b9a2086f94b69b3c67f50))
* check-rust encodes is_zero ([#1015](https://github.com/assura-lang/assura/issues/1015)) ([f42c3ff](https://github.com/assura-lang/assura/commit/f42c3ffd19712e852aba95c800c2b385f4273bed))
* check-rust encodes saturating_mul ([#1012](https://github.com/assura-lang/assura/issues/1012)) ([27546b0](https://github.com/assura-lang/assura/commit/27546b0b79926120d4ce0014792497982f5f7c23))
* check-rust encodes simple if bodies ([#986](https://github.com/assura-lang/assura/issues/986)) ([#990](https://github.com/assura-lang/assura/issues/990)) ([843c072](https://github.com/assura-lang/assura/commit/843c072e7e3fa963c2b3afe30b959bbaf72e28db))
* check-rust encodes simple match bodies as IR ([#994](https://github.com/assura-lang/assura/issues/994)) ([dc7a807](https://github.com/assura-lang/assura/commit/dc7a807ee30c612dc7a87fcf81a3a3b8f3a3368c))
* check-rust encodes single-let bodies ([#988](https://github.com/assura-lang/assura/issues/988)) ([38cb970](https://github.com/assura-lang/assura/commit/38cb9702c46370288526acb217b34553b9b1ec67))
* check-rust folds lets inside if/match branch blocks ([#997](https://github.com/assura-lang/assura/issues/997)) ([6c0d96a](https://github.com/assura-lang/assura/commit/6c0d96ab490c1ed6d612a080007bc250362c6b7b))
* check-rust folds multi-let bodies into one expression ([#996](https://github.com/assura-lang/assura/issues/996)) ([0de1ccf](https://github.com/assura-lang/assura/commit/0de1ccfcba8bf8f5e17749df7ec5929db58f6161))
* check-rust next_power_of_two and 64-bit const-mask bitops ([#1189](https://github.com/assura-lang/assura/issues/1189)) ([8c9c92a](https://github.com/assura-lang/assura/commit/8c9c92a8db361b4b25cf311b9ec325593b2885c9))
* check-rust rem_euclid with NonZero divisors ([#1195](https://github.com/assura-lang/assura/issues/1195)) ([7da5cec](https://github.com/assura-lang/assura/commit/7da5cecbfa1e900f06f13cd245046a4cb36936ce))
* check-rust rewrites identity match guards to if IR ([#1000](https://github.com/assura-lang/assura/issues/1000)) ([e4148bc](https://github.com/assura-lang/assura/commit/e4148bc3c259971395bd3aecbdbc8d7ddabc6155))
* check-rust signed bit ops (zeros, reverse, swap) ([#1194](https://github.com/assura-lang/assura/issues/1194)) ([cda5e2d](https://github.com/assura-lang/assura/commit/cda5e2d2f25a908e3fd789fa5a7e94360abce340))
* check-rust signed both-var bitops, 64-bit rotate, u64 pot, ilog ([#1175](https://github.com/assura-lang/assura/issues/1175)) ([352870c](https://github.com/assura-lang/assura/commit/352870caf160e3e2cebeabcd4281be5498ac76f5)), closes [#1171](https://github.com/assura-lang/assura/issues/1171) [#1172](https://github.com/assura-lang/assura/issues/1172) [#1173](https://github.com/assura-lang/assura/issues/1173) [#1174](https://github.com/assura-lang/assura/issues/1174)
* check-rust signed count_ones + encode surface docs ([#1193](https://github.com/assura-lang/assura/issues/1193)) ([214e612](https://github.com/assura-lang/assura/commit/214e612a6e8f7d578f1fc9c6594cb541555802e2))
* check-rust trailing_ones/leading_ones for path params ([#1196](https://github.com/assura-lang/assura/issues/1196)) ([134259b](https://github.com/assura-lang/assura/commit/134259b5ba753d24c7683e6d16f12191a42f82d3))
* check-rust u8/u16/u32 ranges and saturating ops ([#1011](https://github.com/assura-lang/assura/issues/1011)) ([d0f2a97](https://github.com/assura-lang/assura/commit/d0f2a97632d3a2a70568aa4f96fca63d2ded1290))
* check-rust variable isqrt for u8/u16 path params ([#1192](https://github.com/assura-lang/assura/issues/1192)) ([ac9803a](https://github.com/assura-lang/assura/commit/ac9803abc75545d2dccb28931c6c51ff36cf5e42))
* complete checked_*.is_some()/is_none() for unwrap_or surface ([#1358](https://github.com/assura-lang/assura/issues/1358)) ([cd9d775](https://github.com/assura-lang/assura/commit/cd9d775b1c354e54cf0e18f8809e19fa5a7116d2))
* distribute if/match over both binary sides ([#1336](https://github.com/assura-lang/assura/issues/1336)) ([529fe89](https://github.com/assura-lang/assura/commit/529fe892912e379c78799afa11d57b7f2bac6f60))
* dual-track onboarding (IR synth + wrapping_div/rem) ([#1363](https://github.com/assura-lang/assura/issues/1363)) ([29e9c98](https://github.com/assura-lang/assura/commit/29e9c9830b0f0f531d13c6cb6218943d43511e9c))
* encode both-variable unsigned BitAnd/Or/Xor (bits&lt;=32) ([#1167](https://github.com/assura-lang/assura/issues/1167)) ([aaabdc0](https://github.com/assura-lang/assura/commit/aaabdc0349d3be266884ca6de45caca160c18cee))
* encode checked_*.is_some()/is_none() overflow bounds ([#1357](https://github.com/assura-lang/assura/issues/1357)) ([4b59709](https://github.com/assura-lang/assura/commit/4b597091c191038d94816a92dd27ed807ddc8dd1))
* encode checked_abs().unwrap_or ([#1347](https://github.com/assura-lang/assura/issues/1347)) ([912baba](https://github.com/assura-lang/assura/commit/912babaabb820fd98dd779f2cda05df99e475cdf))
* encode checked_add/sub(const).unwrap_or ([#1338](https://github.com/assura-lang/assura/issues/1338)) ([b31e77a](https://github.com/assura-lang/assura/commit/b31e77a10b79c8adcac010ec450e2287cd408597))
* encode checked_div/rem(const).unwrap_or ([#1342](https://github.com/assura-lang/assura/issues/1342)) ([b7f8e85](https://github.com/assura-lang/assura/commit/b7f8e85ea4cae9b3625c90ce5b34dc628a542656))
* encode checked_ilog2/ilog10().unwrap_or ([#1348](https://github.com/assura-lang/assura/issues/1348)) ([09dfb47](https://github.com/assura-lang/assura/commit/09dfb478da927b50580296f8285a49a17dfda978))
* encode checked_mul(const).unwrap_or ([#1340](https://github.com/assura-lang/assura/issues/1340)) ([1f28e56](https://github.com/assura-lang/assura/commit/1f28e562796423b728a5c68da35c4934576a3917))
* encode checked_neg().unwrap_or ([#1344](https://github.com/assura-lang/assura/issues/1344)) ([88a7807](https://github.com/assura-lang/assura/commit/88a7807ddd4f7f146e4ddafcc2b4143976700623))
* encode checked_next_power_of_two/shl/shr and overflowing_pow peels ([#1351](https://github.com/assura-lang/assura/issues/1351)) ([5339a82](https://github.com/assura-lang/assura/commit/5339a82e70b6807317b98b23b44bbc634baf3354))
* encode checked_pow(0..=4).unwrap_or ([#1345](https://github.com/assura-lang/assura/issues/1345)) ([6e65e68](https://github.com/assura-lang/assura/commit/6e65e6842e03a9d2870563619609f0278ce6cde7))
* encode i64 wrapping_* via synthetic 2^64 modulus ([#1128](https://github.com/assura-lang/assura/issues/1128)) ([9789ac0](https://github.com/assura-lang/assura/commit/9789ac06461321b6ea6397b3f5905334829643fc)), closes [#1010](https://github.com/assura-lang/assura/issues/1010)
* encode let-if fold (if-over-binary distribute) ([#1334](https://github.com/assura-lang/assura/issues/1334)) ([c8256d9](https://github.com/assura-lang/assura/commit/c8256d96e4ea2d2972ba892e03f9c9b7616569e7))
* encode match arms with plain ident bindings ([#1313](https://github.com/assura-lang/assura/issues/1313)) ([4e04879](https://github.com/assura-lang/assura/commit/4e04879ceb599b0ab91d7525d33cb9cc2671eab8))
* encode nested signed wrapping_neg via modular 2^w ([#1161](https://github.com/assura-lang/assura/issues/1161)) ([5fd70cd](https://github.com/assura-lang/assura/commit/5fd70cda80e6229dd0ff419ee509407f548bb6aa))
* encode overflowing_*(…).0 as wrapping_* ([#1339](https://github.com/assura-lang/assura/issues/1339)) ([1e6d122](https://github.com/assura-lang/assura/commit/1e6d122b0e95377af85045d9dd05ab6812985ec1))
* encode overflowing_*(…).1 overflow flag peels ([#1360](https://github.com/assura-lang/assura/issues/1360)) ([bf26fc3](https://github.com/assura-lang/assura/commit/bf26fc3a7c76bd3bbb74322e591c2cc7f999d24d))
* encode overflowing_shl/shr peels as wrapping_* ([#1356](https://github.com/assura-lang/assura/issues/1356)) ([ef776fd](https://github.com/assura-lang/assura/commit/ef776fd0a0e4c0f5c719bd2635eb12c2161f8c9f))
* encode signed BitAnd/Or/Xor with const mask (bits&lt;=32) ([#1166](https://github.com/assura-lang/assura/issues/1166)) ([131d35e](https://github.com/assura-lang/assura/commit/131d35ea11651eec607c440e36683dc700468e50))
* encode signed i64 bit peels via synthetic 2^64 ([#1353](https://github.com/assura-lang/assura/issues/1353)) ([2f38e9b](https://github.com/assura-lang/assura/commit/2f38e9b913cdb0564d1a12afcc3f2cf087df9965))
* encode signed i64 both-variable bitops via synthetic 2^64 ([#1355](https://github.com/assura-lang/assura/issues/1355)) ([d120131](https://github.com/assura-lang/assura/commit/d1201312f044a76041e4375368af890b80263a9e))
* encode signed next_multiple_of via rem_euclid formula ([#1158](https://github.com/assura-lang/assura/issues/1158)) ([9974176](https://github.com/assura-lang/assura/commit/99741767b503134d964cfc3890a6c0d6c4603526))
* encode signed path-param ilog10 for check-rust ([#1233](https://github.com/assura-lang/assura/issues/1233)) ([b51af20](https://github.com/assura-lang/assura/commit/b51af20ff8240fd350212bb0ac6408c17080363f))
* encode signed path-param ilog2 for check-rust ([#1232](https://github.com/assura-lang/assura/issues/1232)) ([030d6bb](https://github.com/assura-lang/assura/commit/030d6bbba51ea6aadd4ba7e227c854199ead1cde))
* encode signed rem_euclid/div_euclid with positive const ([#1154](https://github.com/assura-lang/assura/issues/1154)) ([5de2a68](https://github.com/assura-lang/assura/commit/5de2a6845ec238900558b5499980b4a36da376ee))
* encode signed rotate_left/right by const ([#1134](https://github.com/assura-lang/assura/issues/1134)) ([7903d3e](https://github.com/assura-lang/assura/commit/7903d3e9b16b0485b8e6bc812fa5fc5019109d8c))
* encode signed wrapping_shl by const (i8..i64) ([#1131](https://github.com/assura-lang/assura/issues/1131)) ([72d1b55](https://github.com/assura-lang/assura/commit/72d1b559cf89bbf470c8b12addef8a74d2aaba1c))
* encode signed wrapping_shr via floor div by 2^k ([#1146](https://github.com/assura-lang/assura/issues/1146)) ([119bd8b](https://github.com/assura-lang/assura/commit/119bd8b5de6e5181885fa380e8632f60c3a99f64)), closes [#1144](https://github.com/assura-lang/assura/issues/1144)
* encode simple Rust bodies in check-rust ([#975](https://github.com/assura-lang/assura/issues/975)) ([#983](https://github.com/assura-lang/assura/issues/983)) ([d43a251](https://github.com/assura-lang/assura/commit/d43a251e747469098ad25c5fdd86f761bb0159d7))
* encode u32 isqrt via binary search for check-rust ([#1272](https://github.com/assura-lang/assura/issues/1272)) ([c0e2746](https://github.com/assura-lang/assura/commit/c0e27468377db563f9505bbc7d60992b98308ba5))
* encode u64 both-variable bitops for check-rust ([#1266](https://github.com/assura-lang/assura/issues/1266)) ([c216bc3](https://github.com/assura-lang/assura/commit/c216bc38ecaf290647b5f503047005ac5c42dbf3))
* encode u64 next_power_of_two for check-rust ([#1265](https://github.com/assura-lang/assura/issues/1265)) ([9a3480a](https://github.com/assura-lang/assura/commit/9a3480a2e795962eee0b12f7c2ac4536427aa7ad))
* encode u64 path-param ilog10 for check-rust ([#1277](https://github.com/assura-lang/assura/issues/1277)) ([1ca24bf](https://github.com/assura-lang/assura/commit/1ca24bf2f81af22cf7374a7c189f08b36767c888))
* encode u64 path-param ilog2 for check-rust ([#1276](https://github.com/assura-lang/assura/issues/1276)) ([c69e6ee](https://github.com/assura-lang/assura/commit/c69e6eec588b1cd7ea44d8bc362e825abab515e3))
* encode u64 path-param isqrt for check-rust ([#1283](https://github.com/assura-lang/assura/issues/1283)) ([9d032b3](https://github.com/assura-lang/assura/commit/9d032b350fc832b935c9bdf226363bf6dfd39129))
* encode u64 saturating_add/sub/mul for check-rust ([#1285](https://github.com/assura-lang/assura/issues/1285)) ([2191892](https://github.com/assura-lang/assura/commit/2191892e641bf0aa1405d1725b300bf79abc7e7a))
* encode u64 trailing/leading zeros, reverse, and bitnot ([#1274](https://github.com/assura-lang/assura/issues/1274)) ([d2725f6](https://github.com/assura-lang/assura/commit/d2725f6ae0a63bdc96bc9b03bf929a2694f263da))
* encode u64::MAX and u64::MIN for check-rust ([#1288](https://github.com/assura-lang/assura/issues/1288)) ([07ee574](https://github.com/assura-lang/assura/commit/07ee5744eb6937e84786ef67f8eb3fc0cdac1e94))
* encode u64/i64 count_ones and count_zeros for check-rust ([#1273](https://github.com/assura-lang/assura/issues/1273)) ([f6d1227](https://github.com/assura-lang/assura/commit/f6d12274fe6dd92114d274c14571a2436851f6ba))
* encode u64/usize wrapping_shl/shr via synthetic 2^64 ([#1163](https://github.com/assura-lang/assura/issues/1163)) ([144a5ba](https://github.com/assura-lang/assura/commit/144a5ba275bd4dc8273889a7b3b9a01b2a5896ab))
* encode unsigned path-param count_ones via bit-sum ([#1132](https://github.com/assura-lang/assura/issues/1132)) ([6a0c5cc](https://github.com/assura-lang/assura/commit/6a0c5ccd39a3b11a5427793336c6794ac21386f6))
* encode unsigned path-param count_zeros via bits-ones ([#1133](https://github.com/assura-lang/assura/issues/1133)) ([4d551db](https://github.com/assura-lang/assura/commit/4d551db9967c68007071b8179fe36dd0da025a4d))
* encode unsigned path-param leading_zeros ([#1138](https://github.com/assura-lang/assura/issues/1138)) ([83a5223](https://github.com/assura-lang/assura/commit/83a5223b40f24527fb62d5af5c8e45671c2863f9))
* encode unsigned path-param reverse_bits ([#1140](https://github.com/assura-lang/assura/issues/1140)) ([166bbb0](https://github.com/assura-lang/assura/commit/166bbb0c84e906c307e3ad9e334d1dcdf2706428))
* encode unsigned path-param swap_bytes ([#1141](https://github.com/assura-lang/assura/issues/1141)) ([4f367f4](https://github.com/assura-lang/assura/commit/4f367f47944dc5d38e8069d9b11d4213bc025967))
* encode unsigned path-param trailing_zeros ([#1137](https://github.com/assura-lang/assura/issues/1137)) ([6a1f232](https://github.com/assura-lang/assura/commit/6a1f232cfe1722a709a5745a2f666e1b8f63ab81))
* encode variable BitAnd/Or/Xor with const mask (unsigned ≤32) ([#1165](https://github.com/assura-lang/assura/issues/1165)) ([218b93f](https://github.com/assura-lang/assura/commit/218b93f7987639c5b72f0c6ba05d3dad836839f9))
* encode variable bitwise NOT for fixed-width ints (bits&lt;=32) ([#1168](https://github.com/assura-lang/assura/issues/1168)) ([7d94157](https://github.com/assura-lang/assura/commit/7d941572608cdf52a2970c06e51630d4a529809c))
* encode variable i64 wrapping_shl/shr via 64-case sum ([#1159](https://github.com/assura-lang/assura/issues/1159)) ([2541233](https://github.com/assura-lang/assura/commit/2541233a2b542f280120489f32a2168e9a5ab20a)), closes [#1151](https://github.com/assura-lang/assura/issues/1151)
* encode variable rotate_left/right via case-sum (bits&lt;=32) ([#1164](https://github.com/assura-lang/assura/issues/1164)) ([44d9dbb](https://github.com/assura-lang/assura/commit/44d9dbbcae14a46e5a8d3629b43d2f5ff90b67b3))
* encode variable wrapping_shl/shr for u8/u16 ([#1147](https://github.com/assura-lang/assura/issues/1147)) ([9ef212d](https://github.com/assura-lang/assura/commit/9ef212dd67d7509a31cc4f5baf13263aebfcc311)), closes [#1145](https://github.com/assura-lang/assura/issues/1145)
* encode wrapping_pow with small const exponent ([#1290](https://github.com/assura-lang/assura/issues/1290)) ([0039bd0](https://github.com/assura-lang/assura/commit/0039bd0de9d502a0d4e918dadc3eedc9228505b9))
* expand check-rust body IR encoding ([#984](https://github.com/assura-lang/assura/issues/984)) ([aa6f94a](https://github.com/assura-lang/assura/commit/aa6f94a9cf211e51efa314c0f632a534a6ddf531))
* file_info.ir in check --json + peel/synth tests ([#1374](https://github.com/assura-lang/assura/issues/1374)) ([5877e9f](https://github.com/assura-lang/assura/commit/5877e9faea5cca910ae274c500dab3d1ab7d6a3e))
* fold pure let mut in check-rust body IR ([#1343](https://github.com/assura-lang/assura/issues/1343)) ([90dff94](https://github.com/assura-lang/assura/commit/90dff942d71aeed12fcbf037c8fc722915123b34))
* i64 param ranges + saturating_add/sub body IR ([#1008](https://github.com/assura-lang/assura/issues/1008)) ([66d4f20](https://github.com/assura-lang/assura/commit/66d4f20a8f23deb674690153b3930877935a8788))
* multi-ensures + method peels for richer IR synthesis ([#1368](https://github.com/assura-lang/assura/issues/1368)) ([6344bcf](https://github.com/assura-lang/assura/commit/6344bcfaeb6b4bd09e139b4fc886e4e3ab3db2f9))
* nested is_power_of_two via expr bounds + wrapping width fallback ([#1169](https://github.com/assura-lang/assura/issues/1169)) ([e374025](https://github.com/assura-lang/assura/commit/e374025fb9c4a917382ffe5b589bf00eb2de2060))
* nested result-bound And + verbose synthesized IR ([#1367](https://github.com/assura-lang/assura/issues/1367)) ([73951e3](https://github.com/assura-lang/assura/commit/73951e3e0220adf9d87a40f1c6dad65f5eeb1d62))
* peel checked_*.unwrap_or_default as unwrap_or(0) ([#1361](https://github.com/assura-lang/assura/issues/1361)) ([6abbce7](https://github.com/assura-lang/assura/commit/6abbce70c3f2b90318b4fa95a0a85c3382788c43))
* peel outer &/* before multi-block if encode ([#1337](https://github.com/assura-lang/assura/issues/1337)) ([13ad5b4](https://github.com/assura-lang/assura/commit/13ad5b415318aabc17f4138e4de229e1e274ce9a))
* pot enum through identity peels; i64 wrap mul CE ([#1129](https://github.com/assura-lang/assura/issues/1129)) ([8aaa246](https://github.com/assura-lang/assura/commit/8aaa246224c5189dc2f66347fa7d17dab344d111))
* result bound And synthesis + synthesis-first onboarding docs ([#1366](https://github.com/assura-lang/assura/issues/1366)) ([21f881b](https://github.com/assura-lang/assura/commit/21f881bfd988d2d068585e7fcb9ae3f54927f1bc))
* signed variable wrapping_shl/shr for width &lt;=32 ([#1149](https://github.com/assura-lang/assura/issues/1149)) ([a6cf538](https://github.com/assura-lang/assura/commit/a6cf53805e66f10c1d9db581e40e8e27f9013153)), closes [#1145](https://github.com/assura-lang/assura/issues/1145)
* synthesize if conditions with &&/||/==&gt; ([#887](https://github.com/assura-lang/assura/issues/887)) ([1994748](https://github.com/assura-lang/assura/commit/19947482031db988c51f700c4af67c3c3b544311))
* synthesize nested if IR for ensures ([#886](https://github.com/assura-lang/assura/issues/886)) ([084e2e9](https://github.com/assura-lang/assura/commit/084e2e9f6069a937270cd33baaab040e9efcd40f)), closes [#885](https://github.com/assura-lang/assura/issues/885)
* variable wrapping_shl/shr for u32; CE e2e ([#1148](https://github.com/assura-lang/assura/issues/1148)) ([2aa24c9](https://github.com/assura-lang/assura/commit/2aa24c9ec263c9d6b14d6aae3ec91421f16eec7e))
* wrap_bounds_for receiver fallback on shl/shr/rotate/neg ([#1170](https://github.com/assura-lang/assura/issues/1170)) ([42e597d](https://github.com/assura-lang/assura/commit/42e597da5d224e373f516f5e3b5a0c98466acf33))


### Bug Fixes

* A03005 catalog, nested tuple IR types, link hygiene ([#906](https://github.com/assura-lang/assura/issues/906)) ([20b9561](https://github.com/assura-lang/assura/commit/20b956172a26e86b9b8aae47707e5707d67ea172))
* agent-instructions --json and coverage no-src UX ([#942](https://github.com/assura-lang/assura/issues/942)) ([eb4edba](https://github.com/assura-lang/assura/commit/eb4edbaacec68003c4c29f3b0533e6da710ff681))
* allow unused_imports in generated Rust ([#959](https://github.com/assura-lang/assura/issues/959)) ([75dd33a](https://github.com/assura-lang/assura/commit/75dd33aad307ad02ffc3d810e09e2db52710664a))
* assura fmt --check --json emits structured status ([#948](https://github.com/assura-lang/assura/issues/948)) ([7af50e1](https://github.com/assura-lang/assura/commit/7af50e178b68665659beb18436719af93e2d5f5c))
* assura fmt accepts stdin via - ([#944](https://github.com/assura-lang/assura/issues/944)) ([85bed95](https://github.com/assura-lang/assura/commit/85bed95414837cb8fd2474be7b32ea2e29d47639))
* assura infer -o emits checkable contract skeletons ([#940](https://github.com/assura-lang/assura/issues/940)) ([59b59a2](https://github.com/assura-lang/assura/commit/59b59a2a4ee9e2c49ad222ec9dbd2657a3a14d03))
* audit --json reports type-error skips ([#953](https://github.com/assura-lang/assura/issues/953)) ([191cbe0](https://github.com/assura-lang/assura/commit/191cbe03f9d2d03e462a6d8e585fe4d05ba8ff42))
* audit --unsafe-only per-fn; pure JSON for suggest-from-crash ([#949](https://github.com/assura-lang/assura/issues/949)) ([5de9d8c](https://github.com/assura-lang/assura/commit/5de9d8c770c5583c2106848ba42accf907fd3abd))
* bind result in property tests from result == ensures ([#932](https://github.com/assura-lang/assura/issues/932)) ([67a9698](https://github.com/assura-lang/assura/commit/67a9698c2afa25df4963831ea5ec59ec6360a609))
* Bool 0/1 prelude and match literal SMT parity ([#889](https://github.com/assura-lang/assura/issues/889)) ([d2ba061](https://github.com/assura-lang/assura/commit/d2ba0619ca4a34f9b2676742bc03609bf9305f10))
* Bool match exhaustiveness; synthesize min/max calls ([#888](https://github.com/assura-lang/assura/issues/888)) ([6c2ca67](https://github.com/assura-lang/assura/commit/6c2ca672fd6dfc739262b52000c34762141524ec))
* check-rust body_not_modeled without co-located IR ([#973](https://github.com/assura-lang/assura/issues/973)) ([72715ce](https://github.com/assura-lang/assura/commit/72715ceb7f8c7528836172c9ec84fbcfd1c6157c))
* check-rust encode if branches with return e; ([#992](https://github.com/assura-lang/assura/issues/992)) ([da7ed71](https://github.com/assura-lang/assura/commit/da7ed71f3ef30705edf10f00f23c412edb02fcff))
* check-rust is_multiple_of refuse zero-including path divisors ([#1204](https://github.com/assura-lang/assura/issues/1204)) ([b0a16c7](https://github.com/assura-lang/assura/commit/b0a16c74c0a73a19f6ac3a717537a9ff1d3124c5))
* check-rust nested if body IR and honest body_not_modeled ([#991](https://github.com/assura-lang/assura/issues/991)) ([16832f9](https://github.com/assura-lang/assura/commit/16832f9d83ed56def0223bb93d3c0d84916c6491))
* check-rust refuse / and % with zero-including path divisors ([#1207](https://github.com/assura-lang/assura/issues/1207)) ([2948a27](https://github.com/assura-lang/assura/commit/2948a27a78baa39c4446608909f8fa39feafbb04))
* clap --json CLI parse errors ([#977](https://github.com/assura-lang/assura/issues/977)) ([#980](https://github.com/assura-lang/assura/issues/980)) ([8b49222](https://github.com/assura-lang/assura/commit/8b49222c35f9d3a51a35b26162bb0e36d1a6e7b6))
* clean temp body-IR sidecars after check-rust ([#989](https://github.com/assura-lang/assura/issues/989)) ([d3cd9ae](https://github.com/assura-lang/assura/commit/d3cd9ae37ef6c49bb60d6d00b366aea512a72c05))
* const-fold bitops for non-neg integer lit operands ([#1119](https://github.com/assura-lang/assura/issues/1119)) ([dd50f6a](https://github.com/assura-lang/assura/commit/dd50f6a14ab7f44c65832b2ee74c256d58d4608f))
* count /// @requires/[@ensures](https://github.com/ensures) as coverage ([#941](https://github.com/assura-lang/assura/issues/941)) ([9f576cd](https://github.com/assura-lang/assura/commit/9f576cde61dcde8201c9b37aa39faa5eab7bce3c))
* coverage --json includes ok and min-coverage status ([#969](https://github.com/assura-lang/assura/issues/969)) ([ccb2c3b](https://github.com/assura-lang/assura/commit/ccb2c3b0c6b386dd3619e8daf8d457903d2a5057))
* coverage maps codegen check() to Assura contracts ([#964](https://github.com/assura-lang/assura/issues/964)) ([74512df](https://github.com/assura-lang/assura/commit/74512dfd0c0d96f67355c0cb5dcb3f8222f4e0b4))
* coverage multi-file contract_*.rs matching ([#965](https://github.com/assura-lang/assura/issues/965)) ([c779d37](https://github.com/assura-lang/assura/commit/c779d3783367e0378f4b305f1ac814163879211a))
* detect multi-module circular imports (A02005) ([#947](https://github.com/assura-lang/assura/issues/947)) ([4191f7e](https://github.com/assura-lang/assura/commit/4191f7e9ac9f31e74cf215397ed4d2d1fc2219e5))
* DRY builtin peels + multi-ensures synth-note diagnostics ([#1372](https://github.com/assura-lang/assura/issues/1372)) ([ed338e0](https://github.com/assura-lang/assura/commit/ed338e04786ca298f588bb88eaca9b229b44e3f3)), closes [#1369](https://github.com/assura-lang/assura/issues/1369) [#1370](https://github.com/assura-lang/assura/issues/1370)
* emit JSON for assura --json mcp failures ([#1359](https://github.com/assura-lang/assura/issues/1359)) ([600ed00](https://github.com/assura-lang/assura/commit/600ed003fa2436678a49b5377ca495273a59e886))
* encode div_ceil and rem_euclid for non-neg + const divisor ([#1107](https://github.com/assura-lang/assura/issues/1107)) ([a8d8c55](https://github.com/assura-lang/assura/commit/a8d8c552e01df04a2e97e51e2c69638ede3d4343))
* encode div_euclid for non-neg receiver and const divisor ([#1112](https://github.com/assura-lang/assura/issues/1112)) ([32913f7](https://github.com/assura-lang/assura/commit/32913f78d91fb4f4aec43af19890c27b8970342b))
* encode IR match via pattern equality; synthesize bool !/&&/|| ([#884](https://github.com/assura-lang/assura/issues/884)) ([5972765](https://github.com/assura-lang/assura/commit/59727658f49f0c0dba25ab579c053a5858dc0c75))
* encode midpoint as floor((a+b)/2) ([#1108](https://github.com/assura-lang/assura/issues/1108)) ([20e3e4b](https://github.com/assura-lang/assura/commit/20e3e4ba6c936be1b26fd4248bace075dcfc570d))
* encode nested signum as clamp to [-1, 1] ([#1061](https://github.com/assura-lang/assura/issues/1061)) ([b5dd0b1](https://github.com/assura-lang/assura/commit/b5dd0b131b743666f6d8bcdd4cd25b7eba1bb841))
* encode next_multiple_of for non-neg + positive const ([#1113](https://github.com/assura-lang/assura/issues/1113)) ([57ad6da](https://github.com/assura-lang/assura/commit/57ad6da77e664a30d597e72c5b48f945a8136640))
* encode signed wrapping_add/sub/mul via mod+reinterpret ([#1103](https://github.com/assura-lang/assura/issues/1103)) ([f48d69b](https://github.com/assura-lang/assura/commit/f48d69b0d2a6756e52e2572c2609d6c3109d4d3f))
* encode top-level wrapping_neg as multi-block if ([#1067](https://github.com/assura-lang/assura/issues/1067)) ([5516154](https://github.com/assura-lang/assura/commit/551615491f9a43b5877360b3a7ce793aad46df12))
* encode u8/u16/u32 is_power_of_two via pot enum ([#1097](https://github.com/assura-lang/assura/issues/1097)) ([71a8328](https://github.com/assura-lang/assura/commit/71a83289413e1a7167075f09300c1764ef91bc48))
* encode unsigned rotate_left/right by const ([#1101](https://github.com/assura-lang/assura/issues/1101)) ([496b6ad](https://github.com/assura-lang/assura/commit/496b6adffd2de6e31c7e6191f0d3688808fb9f48))
* encode unsigned wrapping_add/sub via mod 2^w ([#1091](https://github.com/assura-lang/assura/issues/1091)) ([ccffe65](https://github.com/assura-lang/assura/commit/ccffe65482cd3e1c68d86277e10b6b2e49d31c5f))
* encode unsigned wrapping_mul via mod 2^w ([#1093](https://github.com/assura-lang/assura/issues/1093)) ([c7e32b3](https://github.com/assura-lang/assura/commit/c7e32b3e4e921b5f9f743bf0c7612f94652b71d5))
* encode unsigned wrapping_neg via mod 2^w ([#1094](https://github.com/assura-lang/assura/issues/1094)) ([5301f5c](https://github.com/assura-lang/assura/commit/5301f5ccc2b00bf717f322a41e736a619a0323c9))
* encode unsigned wrapping_shl/shr by const ([#1100](https://github.com/assura-lang/assura/issues/1100)) ([e206ae1](https://github.com/assura-lang/assura/commit/e206ae19f608c8927c727ab17eead82b819ec274))
* expand minified one-line sources in assura fmt ([#936](https://github.com/assura-lang/assura/issues/936)) ([ffb21a1](https://github.com/assura-lang/assura/commit/ffb21a180f60ee1f29dc9d5edf228f0a97f75458)), closes [#919](https://github.com/assura-lang/assura/issues/919)
* fold multi-let through Reference and Cast in body IR ([#1024](https://github.com/assura-lang/assura/issues/1024)) ([f86ba71](https://github.com/assura-lang/assura/commit/f86ba7104139fa8c28a606566cc62f55f466eee6))
* honor --json for assura infer ([#946](https://github.com/assura-lang/assura/issues/946)) ([8412aef](https://github.com/assura-lang/assura/commit/8412aef8a0ec89a88808a964f4c66de73cc29d70))
* honor --json for doc/ir-prompt/test-gen; infer -o fallback ([#943](https://github.com/assura-lang/assura/issues/943)) ([89609ab](https://github.com/assura-lang/assura/commit/89609abe66c5002f7619b1f62b15ce4c73a823c4))
* honor global --json for diff, coverage, and audit ([#925](https://github.com/assura-lang/assura/issues/925)) ([0547e98](https://github.com/assura-lang/assura/commit/0547e98da28d170a16eca7eb28b54d13dd60bbcb))
* honor global --json for explain and doctor ([#934](https://github.com/assura-lang/assura/issues/934)) ([f46ec2d](https://github.com/assura-lang/assura/commit/f46ec2d12c4139b69fc0acaa7d31c4018ae428d4))
* human-readable diff and JSON project check ([#921](https://github.com/assura-lang/assura/issues/921)) ([19276f9](https://github.com/assura-lang/assura/commit/19276f9814226e3b925e6856cb12c693b4de4c86))
* include stats in check --json when --stats is set ([#970](https://github.com/assura-lang/assura/issues/970)) ([109577d](https://github.com/assura-lang/assura/commit/109577d7a304888a534a9dd3498b98687b15be9f))
* init SafeDivision template with IR-backed ensures ([#931](https://github.com/assura-lang/assura/issues/931)) ([868512c](https://github.com/assura-lang/assura/commit/868512c81f6762efa59f643f79961e556e18288a)), closes [#920](https://github.com/assura-lang/assura/issues/920)
* inject multi-block co-located IR into codegen ([#883](https://github.com/assura-lang/assura/issues/883)) ([52ed92c](https://github.com/assura-lang/assura/commit/52ed92c82857c345fbaa8ccf964564fb3b469db2))
* ir --json write purity and wrapping_abs peel ([#1362](https://github.com/assura-lang/assura/issues/1362)) ([a300900](https://github.com/assura-lang/assura/commit/a30090098faa1a959650455aeb6bb22c18f8c966))
* is_builtin clamp/signum + faster -v IR listing ([#1373](https://github.com/assura-lang/assura/issues/1373)) ([45f7561](https://github.com/assura-lang/assura/commit/45f75616b08874516bccb66ea81f4ad4cc4d885d))
* match-guard rewrite allows non-identity arm bodies ([#1002](https://github.com/assura-lang/assura/issues/1002)) ([60e1d90](https://github.com/assura-lang/assura/commit/60e1d90c3c922b4dc3a7a488f98fdfae0c859612))
* MPI cycle 2 (completions JSON + honest body_not_modeled) ([#978](https://github.com/assura-lang/assura/issues/978)) ([787b2b1](https://github.com/assura-lang/assura/commit/787b2b1e585ae2dc503717b9be49a1864078ae86))
* MPI cycle 3 (watch/format --json purity) ([#979](https://github.com/assura-lang/assura/issues/979)) ([de51921](https://github.com/assura-lang/assura/commit/de519218bd14c9feb7522ce7d2f10b932d2de7b4))
* multi-ensures result== preference + synthesis-first docs ([#1371](https://github.com/assura-lang/assura/issues/1371)) ([3df1f47](https://github.com/assura-lang/assura/commit/3df1f47263b534e45f6dfc56bd364dd25080a83a))
* multi-perspective improve (agent CLI purity + docs) ([#976](https://github.com/assura-lang/assura/issues/976)) ([f46951a](https://github.com/assura-lang/assura/commit/f46951ad52935986407bf3c7e38b2e76bc8cd975))
* multi-token enum variant field types ([#916](https://github.com/assura-lang/assura/issues/916)) ([76ea6db](https://github.com/assura-lang/assura/commit/76ea6dbbbc2b9bea470c3163d83e5eab9aa781ef))
* nested and unary arithmetic IR synthesis ([#880](https://github.com/assura-lang/assura/issues/880)) ([a68b69d](https://github.com/assura-lang/assura/commit/a68b69d6a082c9296629f1c18975990fba5bf71b))
* nested empty tuples and fn param tuple types ([#913](https://github.com/assura-lang/assura/issues/913)) ([bff175d](https://github.com/assura-lang/assura/commit/bff175dce715f7f4e7534f013a0b9f29efd99019))
* nested tuple field chains and t.1 e2e coverage ([#902](https://github.com/assura-lang/assura/issues/902)) ([b1a10c5](https://github.com/assura-lang/assura/commit/b1a10c517f8ee14425a69248c8dbd62aea2a73e2))
* nested-struct proptest and safer i64 strategies ([#898](https://github.com/assura-lang/assura/issues/898)) ([c738e70](https://github.com/assura-lang/assura/commit/c738e70a68cda9fcf6b5361a467fe170e7b3cb04))
* newline struct fields and field IR inject ([#895](https://github.com/assura-lang/assura/issues/895)) ([4c18079](https://github.com/assura-lang/assura/commit/4c180799ec13cf94a437c7aee37a357b90d285c8))
* only rewrite default generated/ next to source ([#954](https://github.com/assura-lang/assura/issues/954)) ([cd0694d](https://github.com/assura-lang/assura/commit/cd0694d3ce76dae7a20e265aa38c479c8e1d8fed))
* parse let as atom; synthesize let-binding ensures ([#893](https://github.com/assura-lang/assura/issues/893)) ([e5b108a](https://github.com/assura-lang/assura/commit/e5b108aa617b7768e77d04ec4049897acd627cde))
* parse service requires: state == X as expression ([#930](https://github.com/assura-lang/assura/issues/930)) ([c4ba5d4](https://github.com/assura-lang/assura/commit/c4ba5d4b8ff23a5235e69f98a4664eaeac896a08))
* peel paren when refusing literal zero divisors ([#1066](https://github.com/assura-lang/assura/issues/1066)) ([5a6f772](https://github.com/assura-lang/assura/commit/5a6f7724357d9dd9445750ef3f30eca6d281b4d4))
* peep abs_diff(x, x) to zero ([#1077](https://github.com/assura-lang/assura/issues/1077)) ([d400823](https://github.com/assura-lang/assura/commit/d4008237f9e0e73a9732f31f3d1784ed6ed06425))
* peep abs_diff(x,x) is_zero/is_positive ([#1087](https://github.com/assura-lang/assura/issues/1087)) ([246bc92](https://github.com/assura-lang/assura/commit/246bc9205bdc2e4eb248b2fdb80c6e5b6666c26d))
* peep abs().is_negative() to false ([#1083](https://github.com/assura-lang/assura/issues/1083)) ([e3861d4](https://github.com/assura-lang/assura/commit/e3861d4b67e0ec00b75601dc85751f1db4e63662))
* peep clamp(x, y, y) to y ([#1080](https://github.com/assura-lang/assura/issues/1080)) ([b444410](https://github.com/assura-lang/assura/commit/b444410a97ce661c7055d4d6c59c4532fe569c3c))
* peep const bit counts and shift/rotate by 0 ([#1096](https://github.com/assura-lang/assura/issues/1096)) ([355a517](https://github.com/assura-lang/assura/commit/355a51763ce831b46d5990a35201611afc135f70))
* peep const count_zeros for typed integer lits ([#1102](https://github.com/assura-lang/assura/issues/1102)) ([e80edc0](https://github.com/assura-lang/assura/commit/e80edc0afdeded363e37183aca7045d3aa204816))
* peep const ilog10 and encode unsigned_abs as abs ([#1110](https://github.com/assura-lang/assura/issues/1110)) ([4218c7d](https://github.com/assura-lang/assura/commit/4218c7d870cb0e497058833b101d377957c7472d))
* peep const is_power_of_two (partial [#1034](https://github.com/assura-lang/assura/issues/1034)) ([#1089](https://github.com/assura-lang/assura/issues/1089)) ([70eea50](https://github.com/assura-lang/assura/commit/70eea5077e96abe0beb79be9dc734fc8fd295085))
* peep const isqrt for non-negative integer lits ([#1106](https://github.com/assura-lang/assura/issues/1106)) ([bf8a5be](https://github.com/assura-lang/assura/commit/bf8a5be809d15048f7002b112aa9aa736462a20d))
* peep const next_power_of_two for non-neg lits ([#1105](https://github.com/assura-lang/assura/issues/1105)) ([dd587d0](https://github.com/assura-lang/assura/commit/dd587d0c6ee552e9e427474fc67ef7e4594af984))
* peep const reverse_bits, swap_bytes, ilog2 ([#1098](https://github.com/assura-lang/assura/issues/1098)) ([ec670ae](https://github.com/assura-lang/assura/commit/ec670aeffdc42c3e6cb79be100d74e15042ce65a))
* peep const trailing_ones and typed leading_ones ([#1114](https://github.com/assura-lang/assura/issues/1114)) ([c374b9b](https://github.com/assura-lang/assura/commit/c374b9bbc3c21fc811791bd644780a9683bf8bf0))
* peep const wrapping_next_power_of_two (typed) ([#1117](https://github.com/assura-lang/assura/issues/1117)) ([b97e1fe](https://github.com/assura-lang/assura/commit/b97e1fe4d01bf545e5635acdd0467c38ee89cd5a))
* peep free min/max(x, x) to identity ([#1079](https://github.com/assura-lang/assura/issues/1079)) ([2722a16](https://github.com/assura-lang/assura/commit/2722a16ac186fa6cad17ddbb5f957a17dfc704ad))
* peep is_multiple_of(-1) to true ([#1075](https://github.com/assura-lang/assura/issues/1075)) ([1b5aee5](https://github.com/assura-lang/assura/commit/1b5aee5e258a3e06ac9a38c47e969244c57f7863))
* peep is_multiple_of(1) to true ([#1073](https://github.com/assura-lang/assura/issues/1073)) ([4b70a91](https://github.com/assura-lang/assura/commit/4b70a91184a58b34673b85ab2a644c3ae7ec9138))
* peep min/max of the same path to identity ([#1078](https://github.com/assura-lang/assura/issues/1078)) ([cab10a6](https://github.com/assura-lang/assura/commit/cab10a6b4e17ebc7ce7629c50d75e0c2d5f5f53e))
* peep saturating_abs().is_negative() to false ([#1084](https://github.com/assura-lang/assura/issues/1084)) ([a0625ca](https://github.com/assura-lang/assura/commit/a0625caa8db33f1b4eba400c9b0ed57df20cd347))
* peep typed const bitwise NOT ([#1120](https://github.com/assura-lang/assura/issues/1120)) ([7a97394](https://github.com/assura-lang/assura/commit/7a97394bebee837b75d4241c14d6ea5a55ad7755))
* peep wrapping identity constants ([#1069](https://github.com/assura-lang/assura/issues/1069)) ([32d722b](https://github.com/assura-lang/assura/commit/32d722b91e7aa8bddc2629e3543329c281fd80cc))
* peep wrapping_sub(x, x) to zero ([#1071](https://github.com/assura-lang/assura/issues/1071)) ([378135a](https://github.com/assura-lang/assura/commit/378135adea577926263654246e8bf4c50a79c722))
* portfolio parallel path merges Z3 with CVC5 ([#945](https://github.com/assura-lang/assura/issues/945)) ([8419cb4](https://github.com/assura-lang/assura/commit/8419cb4b2fdd7745a5e37722347ec1d30816ba47))
* print IR inject message once during build ([#957](https://github.com/assura-lang/assura/issues/957)) ([624ad1c](https://github.com/assura-lang/assura/commit/624ad1cc9a73a63475e521cafd431ce6c135ce99))
* pure JSON and honest counts for infer --json ([#956](https://github.com/assura-lang/assura/issues/956)) ([3318615](https://github.com/assura-lang/assura/commit/33186155e02058756b89b1ec6d490fe6dd7fc9f2))
* pure JSON for assura build --json ([#966](https://github.com/assura-lang/assura/issues/966)) ([c9e39c3](https://github.com/assura-lang/assura/commit/c9e39c3099de91c59c313a78bdb70a7b5b76aac3))
* pure JSON for diff missing-file errors ([#971](https://github.com/assura-lang/assura/issues/971)) ([b99db84](https://github.com/assura-lang/assura/commit/b99db84092811f0397f5bde0ea1793113c53d57e))
* pure JSON for fmt --check on directories ([#963](https://github.com/assura-lang/assura/issues/963)) ([d1f59fb](https://github.com/assura-lang/assura/commit/d1f59fb80a5114c013f8b9a99e0f024d3914fe86))
* pure JSON for fmt, ir, check-rust, ir-prompt, suggest-from-crash ([#972](https://github.com/assura-lang/assura/issues/972)) ([5a63ac0](https://github.com/assura-lang/assura/commit/5a63ac05ae8d4ab19e09c045021db4f29a5a10e8))
* pure JSON for init/audit/infer; doctor -q ([#967](https://github.com/assura-lang/assura/issues/967)) ([ac65cd0](https://github.com/assura-lang/assura/commit/ac65cd036999a08fe16c3106f45324a44a1ed50e))
* pure JSON for test-gen, doc, and REPL ([#968](https://github.com/assura-lang/assura/issues/968)) ([7a1ead1](https://github.com/assura-lang/assura/commit/7a1ead170b98db9071d0bc481fb3b5732fd5f940))
* pure JSON when ir --verify lacks --contract ([#962](https://github.com/assura-lang/assura/issues/962)) ([4074a35](https://github.com/assura-lang/assura/commit/4074a35a1a9258e1c21c6d9b1d710308f41cb85d))
* raise pot enum to 63 so i64 is_power_of_two encodes ([#1099](https://github.com/assura-lang/assura/issues/1099)) ([4b66aad](https://github.com/assura-lang/assura/commit/4b66aad677a832ce410f300b2113002fb62be475))
* refuse body IR for literal div/mod by zero ([#1064](https://github.com/assura-lang/assura/issues/1064)) ([f55ed41](https://github.com/assura-lang/assura/commit/f55ed41caadf1657ec74100fea567a9d3d5096d7))
* reject empty requires/ensures clause bodies (A03006) ([#929](https://github.com/assura-lang/assura/issues/929)) ([87494e1](https://github.com/assura-lang/assura/commit/87494e1b48c8fb94e91f1e881602364aff5c52a3))
* reject empty tuple types and upgrade rmcp 2.2 ([#910](https://github.com/assura-lang/assura/issues/910)) ([bb3a092](https://github.com/assura-lang/assura/commit/bb3a0920dd5479bbb5bb2299d0ad04ec9844c8a5))
* reject empty tuple types on outputs ([#911](https://github.com/assura-lang/assura/issues/911)) ([4f04e52](https://github.com/assura-lang/assura/commit/4f04e52b0ddc327daaa53bfeb07fd809b5673752))
* reject empty tuple types on services and returns ([#912](https://github.com/assura-lang/assura/issues/912)) ([d297a40](https://github.com/assura-lang/assura/commit/d297a406b67c3426aca0d80b10503fc7b5a0681b))
* reject empty tuples in type aliases and refined bases ([#915](https://github.com/assura-lang/assura/issues/915)) ([6e66d16](https://github.com/assura-lang/assura/commit/6e66d166d981cf223fed4169996f6821be477b17))
* reject invalid assura diff --format values ([#933](https://github.com/assura-lang/assura/issues/933)) ([14eaa90](https://github.com/assura-lang/assura/commit/14eaa9050ce8aea89ac81aa165d3d091002b4563))
* reject invalid init names and out-of-range --layer ([#924](https://github.com/assura-lang/assura/issues/924)) ([771b45b](https://github.com/assura-lang/assura/commit/771b45b9b0cee1f7b1c4b042006f7e6deb00b945))
* reject match patterns with wrong constructor field count ([#918](https://github.com/assura-lang/assura/issues/918)) ([883c724](https://github.com/assura-lang/assura/commit/883c7241d07bab19fb22415f383a6ba2ed13f36d))
* reject stub IR on load/write; abs and bool comparison synthesis ([#881](https://github.com/assura-lang/assura/issues/881)) ([787afba](https://github.com/assura-lang/assura/commit/787afbabbc66327ce0268430c2d2c12c873e6ac5))
* repl bare help/quit under --json ([#982](https://github.com/assura-lang/assura/issues/982)) ([51aa178](https://github.com/assura-lang/assura/commit/51aa178676b433620e8f85321e07a1e8188340a4))
* report unresolved imports as A02006, fail project check ([#926](https://github.com/assura-lang/assura/issues/926)) ([c9cb77b](https://github.com/assura-lang/assura/commit/c9cb77b275693a46f969e1781c9dde5fbe10937b))
* saturating_* clamps to return type width ([#1009](https://github.com/assura-lang/assura/issues/1009)) ([d24c387](https://github.com/assura-lang/assura/commit/d24c38758d4beb43f7b9baabfd4940c47a78b377))
* show A01001 lexer errors in human check output ([#958](https://github.com/assura-lang/assura/issues/958)) ([aebd3ba](https://github.com/assura-lang/assura/commit/aebd3ba8f2c2d3c767f70079a29bc9213aa401f6))
* showcase-only matches declared module names ([#923](https://github.com/assura-lang/assura/issues/923)) ([2d5e8d6](https://github.com/assura-lang/assura/commit/2d5e8d60e832e60c37ccd84ffcba388de31f7f4d))
* showcase-only vacuous + agent check-rust body docs ([#987](https://github.com/assura-lang/assura/issues/987)) ([cc797e9](https://github.com/assura-lang/assura/commit/cc797e97efa079a72033b372db92815928c989ae))
* signed wrapping_mul via double-mod for all fixed widths ([#1111](https://github.com/assura-lang/assura/issues/1111)) ([95bd29c](https://github.com/assura-lang/assura/commit/95bd29c5a164dc142de21e2d7a329820857aa854))
* single-element tuple types and MPI polish ([#908](https://github.com/assura-lang/assura/issues/908)) ([d3fb8aa](https://github.com/assura-lang/assura/commit/d3fb8aa8575f4e29c498c55340c9abd4d2515914))
* SMT verify on project-mode check ([#952](https://github.com/assura-lang/assura/issues/952)) ([d115206](https://github.com/assura-lang/assura/commit/d115206bccd5ad344dd9952cc810ad938777bf7b))
* stdin for check and dedupe counterexample result ([#922](https://github.com/assura-lang/assura/issues/922)) ([5e07ba7](https://github.com/assura-lang/assura/commit/5e07ba7ab7af81853e6493b0af0c0d3780483fcf))
* suggest-from-crash --json reports LLM errors ([#955](https://github.com/assura-lang/assura/issues/955)) ([2ba582c](https://github.com/assura-lang/assura/commit/2ba582c1bd76b6075d59bf0671d3cf04f3331ee6))
* synthesize IR for result == x + 1 (param+literal arith) ([#878](https://github.com/assura-lang/assura/issues/878)) ([28f6328](https://github.com/assura-lang/assura/commit/28f6328be9e1891c3b664dd18965dadb48017ee8))
* synthesize nested Bool ensures (&&/||/!/=&gt;) ([#890](https://github.com/assura-lang/assura/issues/890)) ([b42a400](https://github.com/assura-lang/assura/commit/b42a4001a3acfc0be695805f884edd9b6c1592c0))
* synthesize nested field loads (o.inner.v) ([#897](https://github.com/assura-lang/assura/issues/897)) ([b1b2bf8](https://github.com/assura-lang/assura/commit/b1b2bf8f31d8ff7190f3d220029e909dc222c24a)), closes [#896](https://github.com/assura-lang/assura/issues/896)
* synthesize result == xs.length() as IR call length ([#900](https://github.com/assura-lang/assura/issues/900)) ([436c388](https://github.com/assura-lang/assura/commit/436c388f240568ac9f282237b803e754c0f4cb12))
* test-gen --json write errors + CONTRIBUTING purity note ([#981](https://github.com/assura-lang/assura/issues/981)) ([fbb24f8](https://github.com/assura-lang/assura/commit/fbb24f83daa33012d20e91b8d0b3c9311fc6d22f))
* test-gen proptest strategies; pure ir --json ([#950](https://github.com/assura-lang/assura/issues/950)) ([8c6a1a5](https://github.com/assura-lang/assura/commit/8c6a1a5dc5fad4bd0e88cc8bdea280344a4f643e))
* use A02010 for unresolved imports (A02006 is duplicate) ([#927](https://github.com/assura-lang/assura/issues/927)) ([5c0e907](https://github.com/assura-lang/assura/commit/5c0e90752bb2656f8665a97dec7c412ee7a914b0))
* validate --format human|json for coverage and audit ([#935](https://github.com/assura-lang/assura/issues/935)) ([40a1e62](https://github.com/assura-lang/assura/commit/40a1e62d6a4da40576d0628bf6bee1bbf806559e))
* zero-warning flagship demos (length nonneg + determinism) ([#1375](https://github.com/assura-lang/assura/issues/1375)) ([0cba8a0](https://github.com/assura-lang/assura/commit/0cba8a04bbef136310adcac36e0cbda7926d9e82))

## [0.3.0](https://github.com/assura-lang/assura/compare/v0.2.0...v0.3.0) (2026-07-07)


### Features

* co-publish assura CLI and frontends to crates.io ([#845](https://github.com/assura-lang/assura/issues/845)) ([b651fc2](https://github.com/assura-lang/assura/commit/b651fc2ab8c7f7deffb9d4dbf1412114fc6c9885)), closes [#838](https://github.com/assura-lang/assura/issues/838)
* P1/P2 proof equating, offline IR, bin, strict, multi-contract ([#870](https://github.com/assura-lang/assura/issues/870)) ([9a724a5](https://github.com/assura-lang/assura/commit/9a724a53f9fdedad476a4660b1002ec98e9ebb41))


### Bug Fixes

* address GitHub AI code quality findings ([#850](https://github.com/assura-lang/assura/issues/850)) ([ebf9b8d](https://github.com/assura-lang/assura/commit/ebf9b8d5782fc62d6bcf8330cea223960294792d))
* CVC5 signed BV order for fixed-width I* types ([#860](https://github.com/assura-lang/assura/issues/860)) ([d06ab67](https://github.com/assura-lang/assura/commit/d06ab67cecd332b660f0e0448b3894a0666a7a39))
* signed BV comparisons for fixed-width I* types ([#859](https://github.com/assura-lang/assura/issues/859)) ([927063c](https://github.com/assura-lang/assura/commit/927063c4ce4b723e98df608a715c2201160c5013))
* SMT/types/IR batch (fixed-width, match, verify_ir, evolution) ([#856](https://github.com/assura-lang/assura/issues/856)) ([0b0d6eb](https://github.com/assura-lang/assura/commit/0b0d6ebdc1736936041e3a69b5eda6eba3177435))

## [Unreleased]

### Changed

* deps: upgrade rmcp 2.1 → 2.2 in assura-mcp (#907)

## [0.2.0](https://github.com/assura-lang/assura/compare/v0.1.0...v0.2.0) (2026-07-04)


### Features

* register feature_max in resolve; demos use named SMT bounds ([#832](https://github.com/assura-lang/assura/issues/832)) ([fd24fa8](https://github.com/assura-lang/assura/commit/fd24fa8863121905c1fb6b2835ff2cf0aafbcb21))


### Bug Fixes

* distinguish requires-only from empty contracts in check UX ([#822](https://github.com/assura-lang/assura/issues/822)) ([308eac3](https://github.com/assura-lang/assura/commit/308eac3cd4249498fee7777f1bd63f3636e44a36))
* do not fail publish on already-published crates ([#804](https://github.com/assura-lang/assura/issues/804)) ([ee96c5e](https://github.com/assura-lang/assura/commit/ee96c5e5dd9c6c34f5b484c2ad79c04e8dfe215b))
* drop ir_generate expect; document JSON vacuous and driver exclude ([#830](https://github.com/assura-lang/assura/issues/830)) ([a8a30ce](https://github.com/assura-lang/assura/commit/a8a30ced4805221b1de162074ae85765e751c3ce))
* JSON vacuous flags; truncate verification display names ([#828](https://github.com/assura-lang/assura/issues/828)) ([caa1455](https://github.com/assura-lang/assura/commit/caa145577a44b49525b16a16f5cfed4c74082a1c))
* order crates.io publish by all path deps including dev ([#801](https://github.com/assura-lang/assura/issues/801)) ([62662f8](https://github.com/assura-lang/assura/commit/62662f87a6d82bcaa995710a2abcc8141a1ba20d))
* publish-plan order, CI wire-up, vacuous check message ([#818](https://github.com/assura-lang/assura/issues/818)) ([4686a89](https://github.com/assura-lang/assura/commit/4686a89ec0fc7ba0633d5add8f7df3adeb8a18a5))
* retry crates.io 429 and space new crate publishes ([#803](https://github.com/assura-lang/assura/issues/803)) ([e7cc20c](https://github.com/assura-lang/assura/commit/e7cc20c02d5defac2e6f6c7a9cfdd69450334171))
* ship IR prompt templates inside assura-smt for crates.io ([#805](https://github.com/assura-lang/assura/issues/805)) ([3bd06f0](https://github.com/assura-lang/assura/commit/3bd06f08d786e170c55b9e97dc31b36bbd12bbe5))

## 0.1.0 (2026-07-04)


### Bug Fixes

* address issues [#328](https://github.com/assura-lang/assura/issues/328), [#329](https://github.com/assura-lang/assura/issues/329), [#330](https://github.com/assura-lang/assura/issues/330) ([cb9e150](https://github.com/assura-lang/assura/commit/cb9e150158e52a261435ec09b44d98d5293891df))
* address reviewer findings [#707](https://github.com/assura-lang/assura/issues/707)-[#712](https://github.com/assura-lang/assura/issues/712) ([5b165c0](https://github.com/assura-lang/assura/commit/5b165c03d962c82e87126d69c96f8dcc4f1d2029))
* apply cargo fmt to all workspace files ([362bc3b](https://github.com/assura-lang/assura/commit/362bc3b534134252b4f2c94af7bf3b5a9d87c933))
* clean up 5 unused import warnings in assura-smt test files ([f79189e](https://github.com/assura-lang/assura/commit/f79189edc605eec43a05748a623c717837d3127e))
* fmt binop to use binop_str for &&/|| in Assura syntax ([e3fd903](https://github.com/assura-lang/assura/commit/e3fd9038f62db68ce45ecdc1a17fea308fac5933))
* fmt dead code and imports for gate ([94a85a9](https://github.com/assura-lang/assura/commit/94a85a92b62d25e1426fd7737f8d411302f10063))
* force first release-please cut to 0.1.0 ([#782](https://github.com/assura-lang/assura/issues/782)) ([e37ab31](https://github.com/assura-lang/assura/commit/e37ab31c2fc66082cc304d6cc37175a7c1390d7f))
* formatter idempotency, test coverage, and code cleanup ([#682](https://github.com/assura-lang/assura/issues/682)) ([953c6f2](https://github.com/assura-lang/assura/commit/953c6f2f618bb4c33d5768bbd75f8234b96de049))
* MPI cycle 1 — A31007 fairness, vacuous check UX, dead checker cleanup ([#768](https://github.com/assura-lang/assura/issues/768)) ([d300daa](https://github.com/assura-lang/assura/commit/d300daa5853f191ea1b2afda01f3bfe5b9983f48))
* multi-perspective improvement cycle (tests, CI, docs) ([#698](https://github.com/assura-lang/assura/issues/698)) ([d5a3e51](https://github.com/assura-lang/assura/commit/d5a3e51af6668ec1b8a3ff895d5dc8241cd18138))
* multi-perspective improvement cycle 1 ([#646](https://github.com/assura-lang/assura/issues/646)) ([7b9107e](https://github.com/assura-lang/assura/commit/7b9107e1a71850a64c635b95138a4c2e7101e94b))
* ProjectConfig import + SMT-LIB shell-out bugs ([cbb66fb](https://github.com/assura-lang/assura/commit/cbb66fb99352ebcd31324b2514e9c2beca7c7200))
* remove unused SpExpr import after cvc5 migration ([#320](https://github.com/assura-lang/assura/issues/320)) ([49e4264](https://github.com/assura-lang/assura/commit/49e4264adb9bec5d453e13becbf156014a2af73a))
* resolve clippy warnings, fix cargo fmt with cvc5 test module path ([640355d](https://github.com/assura-lang/assura/commit/640355ddb3ed9235c5b1cfdbf35e93e30927206c))
* resolve issues [#316](https://github.com/assura-lang/assura/issues/316), [#320](https://github.com/assura-lang/assura/issues/320), [#321](https://github.com/assura-lang/assura/issues/321), [#322](https://github.com/assura-lang/assura/issues/322) ([6fb29c7](https://github.com/assura-lang/assura/commit/6fb29c77470064c1fd22d0f0bd53ac0590ebe354))
* use canonical type map in check-rust, implement --public-only, document check-rust ([cf2049e](https://github.com/assura-lang/assura/commit/cf2049e225e2309753dcde57384ccccbc70f49b3))
* use simple release-please type for cargo workspace ([#780](https://github.com/assura-lang/assura/issues/780)) ([f703991](https://github.com/assura-lang/assura/commit/f7039910b18e20c5589ee9fa0b465171bd52c56e))
* wire liveness monitor state enums (closes [#770](https://github.com/assura-lang/assura/issues/770)) ([#771](https://github.com/assura-lang/assura/issues/771)) ([8f043cd](https://github.com/assura-lang/assura/commit/8f043cd89d2293c3857c731d635378cff4df6cf0))
* Z3Value soundness fixes, dead code removal, dep bumps ([#515](https://github.com/assura-lang/assura/issues/515)) ([ec58897](https://github.com/assura-lang/assura/commit/ec588970bc18df66f049e7151b48dea122f94064))

### Initial public description (historical notes, 2025-06-14)

Initial release of the Assura compiler.

### Compiler Pipeline

- **Parser**: lexer (logos) + recursive-descent parser (rowan CST) with
  full Pratt expression parsing (8 precedence levels)
- **Name Resolution**: symbol table, scope analysis, cross-reference tracking
- **Type Checker**: 50+ domain-specific checkers covering all 12 feature
  categories (MEM, SEC, TYPE, CONC, NUM, PERF, FMT, STOR, PLAT, TEST, CORE, MISC)
- **SMT Verification**: Z3 backend with Layer 0 (structural) and Layer 1
  (SMT-based) verification; CVC5 fallback; portfolio solver mode
- **Code Generation**: Rust source output via prettyplease; generates
  Cargo workspace with debug_assert! from contracts; proptest generation
  for timeout/unknown results

### CLI Commands

- `assura check` -- full pipeline (parse, resolve, type-check, verify)
  with `--watch`, `--stats`, `--dump-smt`, `--layer`, `--solver` options
- `assura build` -- verify + generate Rust project + cargo check + WASM support
- `assura init` -- scaffold new project with assura.toml and starter contract
- `assura explain` -- look up error codes from 43-entry catalog
- `assura fmt` -- format .assura source files with `--check` mode for CI
- `assura infer` -- generate skeleton bind contracts from Rust source
- `assura test-gen` -- generate proptest code from contracts
- `assura audit` -- scan Rust projects for contract violations

### Language Features

- 195 EBNF grammar productions from the specification
- 10 declaration types: contract, bind, fn, service, type, enum, extern,
  block, prophecy, codec_registry
- Refinement types, linear types, typestate, effect system, taint tracking
- ~278 error codes across 8 categories (A01xxx-A08xxx)
- Watch mode with filesystem notifications and content-hash deduplication

### Editor Support

- VS Code extension with TextMate syntax highlighting and LSP client
- Tree-sitter grammar for editor integration
- LSP server with diagnostics, go-to-definition, hover, completion,
  document symbols, formatting, find references, and rename

### Infrastructure

- CI pipeline with clippy, tests, no-z3 build, generated code check
- 3 demo contracts (libwebp CVE-2023-4863, zlib CVE-2022-37434,
  mbedtls 4x CVSS 9.8 CVEs)
- 50+ must_compile, 30+ must_reject fixture tests, 19 e2e tests

[Unreleased]: https://github.com/assura-lang/assura/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/assura-lang/assura/releases/tag/v0.1.0
