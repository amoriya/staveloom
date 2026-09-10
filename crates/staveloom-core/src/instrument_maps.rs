pub fn get_program_from_sound(id: &str) -> i32 {
    match id {
        "brass.euphonium" => 57,
        "brass.french-horn" => 60,
        "brass.trombone" => 57,
        "brass.trombone.alto" => 57,
        "brass.trombone.bass" => 57,
        "brass.trombone.contrabass" => 57,
        "brass.trombone.tenor" => 57,
        "brass.trumpet" => 56,
        "brass.trumpet.baroque" => 56,
        "brass.trumpet.bass" => 56,
        "brass.trumpet.bflat" => 56,
        "brass.trumpet.c" => 56,
        "brass.trumpet.d" => 56,
        "brass.trumpet.piccolo" => 56,
        "brass.trumpet.pocket" => 56,
        "brass.trumpet.slide" => 56,
        "brass.trumpet.tenor" => 56,
        "brass.tuba" => 58,
        "brass.tuba.bass" => 58,
        "brass.tuba.subcontrabass" => 58,
        "drum.apentemma"
        | "drum.ashiko"
        | "drum.atabaque"
        | "drum.atoke"
        | "drum.atsimevu"
        | "drum.axatse"
        | "drum.bass-drum"
        | "drum.bata"
        | "drum.bata.itotele"
        | "drum.bata.iya"
        | "drum.bata.okonkolo"
        | "drum.bendir"
        | "drum.bodhran"
        | "drum.bombo"
        | "drum.bongo"
        | "drum.bougarabou"
        | "drum.buffalo-drum"
        | "drum.cajon"
        | "drum.chenda"
        | "drum.chu-daiko"
        | "drum.conga"
        | "drum.cuica"
        | "drum.dabakan"
        | "drum.daff"
        | "drum.dafli"
        | "drum.daibyosi"
        | "drum.damroo"
        | "drum.darabuka"
        | "drum.def"
        | "drum.dhol"
        | "drum.dholak"
        | "drum.djembe"
        | "drum.doira"
        | "drum.dondo"
        | "drum.doun-doun-ba"
        | "drum.duff"
        | "drum.dumbek"
        | "drum.fontomfrom"
        | "drum.frame-drum"
        | "drum.frame-drum.arabian"
        | "drum.geduk"
        | "drum.ghatam"
        | "drum.gome"
        | "drum.group"
        | "drum.group.chinese"
        | "drum.group.ewe"
        | "drum.group.indian"
        | "drum.group.set"
        | "drum.hand-drum"
        | "drum.hira-daiko"
        | "drum.ibo"
        | "drum.igihumurizo"
        | "drum.inyahura"
        | "drum.ishakwe"
        | "drum.jang-gu"
        | "drum.kagan"
        | "drum.kakko"
        | "drum.kanjira"
        | "drum.kendhang"
        | "drum.kendhang.ageng"
        | "drum.kendhang.ciblon"
        | "drum.kenkeni"
        | "drum.khol"
        | "drum.kick-drum"
        | "drum.kidi"
        | "drum.ko-daiko"
        | "drum.kpanlogo"
        | "drum.kudum"
        | "drum.lambeg"
        | "drum.lion-drum"
        | "drum.log-drum"
        | "drum.log-drum.african"
        | "drum.log-drum.native"
        | "drum.log-drum.nigerian"
        | "drum.madal"
        | "drum.maddale"
        | "drum.mridangam"
        | "drum.naal"
        | "drum.nagado-daiko"
        | "drum.nagara"
        | "drum.naqara"
        | "drum.o-daiko"
        | "drum.okawa"
        | "drum.okedo-daiko"
        | "drum.pahu-hula"
        | "drum.pakhawaj"
        | "drum.pandeiro"
        | "drum.pandero"
        | "drum.powwow"
        | "drum.pueblo"
        | "drum.repinique"
        | "drum.riq"
        | "drum.rototom"
        | "drum.sabar"
        | "drum.sakara"
        | "drum.sampho"
        | "drum.sangban"
        | "drum.shime-daiko"
        | "drum.slit-drum"
        | "drum.slit-drum.krin"
        | "drum.snare-drum"
        | "drum.snare-drum.electric"
        | "drum.sogo"
        | "drum.surdo"
        | "drum.tabla"
        | "drum.tabla.bayan"
        | "drum.tabla.dayan"
        | "drum.tabor"
        | "drum.taiko"
        | "drum.talking"
        | "drum.tama"
        | "drum.tamborim"
        | "drum.tamborita"
        | "drum.tambourine"
        | "drum.tamte"
        | "drum.tangku"
        | "drum.tan-tan"
        | "drum.taphon"
        | "drum.tar"
        | "drum.tasha"
        | "drum.tenor-drum"
        | "drum.teponaxtli"
        | "drum.thavil"
        | "drum.the-box"
        | "drum.timbale"
        | "drum.tinaja"
        | "drum.toere"
        | "drum.tombak"
        | "drum.tom-tom"
        | "drum.tom-tom.synth"
        | "drum.tsuzumi"
        | "drum.tumbak"
        | "drum.uchiwa-daiko"
        | "drum.udaku"
        | "drum.udu"
        | "drum.zarb" => 128,
        "drum.timpani" => 47,
        "keyboard.harpsichord" => 6,
        "keyboard.piano.electric" => 2,
        "metal.bells.bell-tree" => 128,
        "metal.bells.chimes" => 14,
        "metal.bells.mark-tree" | "metal.bells.wind-chimes" => 300,
        "metal.cymbal.bo"
        | "metal.cymbal.ceng-ceng"
        | "metal.cymbal.chabara"
        | "metal.cymbal.chinese"
        | "metal.cymbal.ching"
        | "metal.cymbal.clash"
        | "metal.cymbal.crash"
        | "metal.cymbal.finger"
        | "metal.cymbal.hand"
        | "metal.cymbal.kesi"
        | "metal.cymbal.manjeera"
        | "metal.cymbal.reverse"
        | "metal.cymbal.ride"
        | "metal.cymbal.sizzle"
        | "metal.cymbal.splash"
        | "metal.cymbal.suspended"
        | "metal.cymbal.tebyoshi"
        | "metal.cymbal.tibetan"
        | "metal.cymbal.tingsha" => 128,
        "metal.gong"
        | "metal.gong.ageng"
        | "metal.gong.agung"
        | "metal.gong.chanchiki"
        | "metal.gong.chinese"
        | "metal.gong.gandingan"
        | "metal.gong.kempul"
        | "metal.gong.kempyang"
        | "metal.gong.ketuk"
        | "metal.gong.kkwenggwari"
        | "metal.gong.luo"
        | "metal.gong.singing"
        | "metal.gong.thai" => 128,
        "metal.triangle" => 128,
        "pitched-percussion.angklung"
        | "pitched-percussion.balafon"
        | "pitched-percussion.bell-lyre"
        | "pitched-percussion.bells"
        | "pitched-percussion.bianqing"
        | "pitched-percussion.bianzhong"
        | "pitched-percussion.bonang"
        | "pitched-percussion.cimbalom"
        | "pitched-percussion.crystal-glasses"
        | "pitched-percussion.dan-tam-thap-luc"
        | "pitched-percussion.fangxiang"
        | "pitched-percussion.gandingan-a-kayo"
        | "pitched-percussion.gangsa"
        | "pitched-percussion.gender"
        | "pitched-percussion.giying"
        | "pitched-percussion.glass-harmonica"
        | "pitched-percussion.glockenspiel"
        | "pitched-percussion.glockenspiel.alto"
        | "pitched-percussion.glockenspiel.soprano"
        | "pitched-percussion.gyil"
        | "pitched-percussion.hammer-dulcimer"
        | "pitched-percussion.handbells"
        | "pitched-percussion.handchimes"
        | "pitched-percussion.kantil"
        | "pitched-percussion.khim"
        | "pitched-percussion.kulintang"
        | "pitched-percussion.kulintang-a-kayo"
        | "pitched-percussion.kulintang-a-tiniok"
        | "pitched-percussion.likembe"
        | "pitched-percussion.luntang"
        | "pitched-percussion.mbira"
        | "pitched-percussion.mbira.array"
        | "pitched-percussion.metallophone"
        | "pitched-percussion.metallophone.alto"
        | "pitched-percussion.metallophone.bass"
        | "pitched-percussion.metallophone.soprano"
        | "pitched-percussion.music-box"
        | "pitched-percussion.pelog-panerus"
        | "pitched-percussion.pemade"
        | "pitched-percussion.penyacah"
        | "pitched-percussion.ranat.ek"
        | "pitched-percussion.ranat.ek-lek"
        | "pitched-percussion.ranat.thum"
        | "pitched-percussion.ranat.thum-lek"
        | "pitched-percussion.reyong"
        | "pitched-percussion.sanza"
        | "pitched-percussion.saron-barung"
        | "pitched-percussion.saron-demong"
        | "pitched-percussion.saron-panerus"
        | "pitched-percussion.slendro-panerus"
        | "pitched-percussion.slentem"
        | "pitched-percussion.tsymbaly"
        | "pitched-percussion.tubes"
        | "pitched-percussion.yangqin" => 9,
        "pitched-percussion.kalimba" => 108,
        "pitched-percussion.marimba" | "pitched-percussion.marimba.bass" => 12,
        "pitched-percussion.tubular-bells" => 14,
        "pitched-percussion.vibraphone" => 11,
        "pitched-percussion.xylophone"
        | "pitched-percussion.xylophone.alto"
        | "pitched-percussion.xylophone.bass"
        | "pitched-percussion.xylophone.soprano"
        | "pitched-percussion.xylorimba" => 13,
        "pluck.guitar"
        | "pluck.guitar.acoustic"
        | "pluck.guitar.nylon-string"
        | "pluck.guitar.pedal-steel"
        | "pluck.guitar.portuguese"
        | "pluck.guitar.requinto"
        | "pluck.guitar.resonator"
        | "pluck.guitar.steel-string" => 24,
        "pluck.ukulele" | "pluck.ukulele.tenor" => 301,
        "rattle.maraca" | "rattle.shaker" => 128,
        "strings.cello" => 42,
        "strings.contrabass" => 43,
        "strings.viola" => 41,
        "strings.violin" => 40,
        "voice.aa"
        | "voice.alto"
        | "voice.aw"
        | "voice.baritone"
        | "voice.bass"
        | "voice.child"
        | "voice.countertenor"
        | "voice.doo"
        | "voice.ee"
        | "voice.female"
        | "voice.kazoo"
        | "voice.male"
        | "voice.mezzo-soprano"
        | "voice.mm"
        | "voice.oo"
        | "voice.percussion"
        | "voice.percussion.beatbox"
        | "voice.soprano"
        | "voice.synth"
        | "voice.talk-box"
        | "voice.tenor"
        | "voice.vocals" => 52,
        "wind.flutes.bansuri"
        | "wind.flutes.blown-bottle"
        | "wind.flutes.calliope"
        | "wind.flutes.danso"
        | "wind.flutes.di-zi"
        | "wind.flutes.dvojnice"
        | "wind.flutes.fife"
        | "wind.flutes.flageolet"
        | "wind.flutes.flute"
        | "wind.flutes.flute.alto"
        | "wind.flutes.flute.bass"
        | "wind.flutes.flute.contra-alto"
        | "wind.flutes.flute.contrabass"
        | "wind.flutes.flute.double-contrabass"
        | "wind.flutes.flute.irish"
        | "wind.flutes.flute.piccolo"
        | "wind.flutes.flute.subcontrabass"
        | "wind.flutes.fujara"
        | "wind.flutes.gemshorn"
        | "wind.flutes.hocchiku"
        | "wind.flutes.hun"
        | "wind.flutes.kaval"
        | "wind.flutes.kawala"
        | "wind.flutes.khlui"
        | "wind.flutes.knotweed"
        | "wind.flutes.koncovka.alto"
        | "wind.flutes.koudi"
        | "wind.flutes.ney"
        | "wind.flutes.nohkan"
        | "wind.flutes.nose"
        | "wind.flutes.overtone.tenor"
        | "wind.flutes.palendag"
        | "wind.flutes.panpipes"
        | "wind.flutes.quena"
        | "wind.flutes.ryuteki"
        | "wind.flutes.shakuhachi"
        | "wind.flutes.shepherds-pipe"
        | "wind.flutes.shinobue"
        | "wind.flutes.shvi"
        | "wind.flutes.suling"
        | "wind.flutes.tarka"
        | "wind.flutes.tumpong"
        | "wind.flutes.venu"
        | "wind.flutes.whistle"
        | "wind.flutes.whistle.alto"
        | "wind.flutes.whistle.low-irish"
        | "wind.flutes.whistle.shiva"
        | "wind.flutes.whistle.slide"
        | "wind.flutes.whistle.tin"
        | "wind.flutes.whistle.tin.bflat"
        | "wind.flutes.whistle.tin.c"
        | "wind.flutes.whistle.tin.d"
        | "wind.flutes.xiao"
        | "wind.flutes.xun" => 73,
        "wind.flutes.ocarina" => 79,
        "wind.flutes.recorder"
        | "wind.flutes.recorder.alto"
        | "wind.flutes.recorder.bass"
        | "wind.flutes.recorder.contrabass"
        | "wind.flutes.recorder.descant"
        | "wind.flutes.recorder.garklein"
        | "wind.flutes.recorder.great-bass"
        | "wind.flutes.recorder.sopranino"
        | "wind.flutes.recorder.soprano"
        | "wind.flutes.recorder.tenor" => 74,
        "wind.reed.bassoon" => 70,
        "wind.reed.clarinet"
        | "wind.reed.clarinet.a"
        | "wind.reed.clarinet.alto"
        | "wind.reed.clarinet.bass"
        | "wind.reed.clarinet.basset"
        | "wind.reed.clarinet.bflat"
        | "wind.reed.clarinet.contra-alto"
        | "wind.reed.clarinet.contrabass"
        | "wind.reed.clarinet.d"
        | "wind.reed.clarinet.eflat"
        | "wind.reed.clarinet.g"
        | "wind.reed.clarinet.piccolo"
        | "wind.reed.clarinet.piccolo.aflat"
        | "wind.reed.clarinette-damour"
        | "wind.reed.saxonette" => 71,
        "wind.reed.melodica" => 302,
        "wind.reed.oboe"
        | "wind.reed.oboe.bass"
        | "wind.reed.oboe.piccolo"
        | "wind.reed.oboe-da-caccia"
        | "wind.reed.oboe-damore" => 68,
        "wind.reed.saxophone" | "wind.reed.saxophone.alto" => 65,
        "wind.reed.saxophone.tenor" => 66,
        "wind.reed.saxophone.baritone"
        | "wind.reed.saxophone.bass"
        | "wind.reed.saxophone.contrabass"
        | "wind.reed.saxophone.subcontrabass" => 67,
        "wind.reed.saxophone.aulochrome"
        | "wind.reed.saxophone.melody"
        | "wind.reed.saxophone.mezzo-soprano"
        | "wind.reed.saxophone.sopranino"
        | "wind.reed.saxophone.sopranissimo"
        | "wind.reed.saxophone.soprano" => 64,
        "wood.castanets" => 303,
        _ => -1,
    }
}

pub fn get_program_from_name(name: &str) -> i32 {
    let s = name.to_lowercase();

    // 1. Specific Percussion
    if s.contains("drum")
        || s.contains("snare")
        || s.contains("cymbals")
        || s.contains("triangle")
        || s.contains("tambourine")
        || s.contains("bell tree")
        || s.contains("janggu")
    {
        128
    } else if s.contains("timpani") {
        47
    } else if s.contains("glockenspiel") || s.contains("bells") {
        9
    } else if s.contains("vibraphone") {
        11
    } else if s.contains("marimba") {
        12
    } else if s.contains("xylophone") {
        13
    } else if s.contains("chime tree") || s.contains("wind chime") {
        300
    } else if s.contains("chime") {
        14
    }
    // 2. Specific Strings
    else if s.contains("string ensemble") || s.contains("strings") {
        48
    } else if s.contains("violin") {
        40
    } else if s.contains("viola") {
        41
    } else if s.contains("cello") {
        42
    } else if s.contains("double bass") || s.contains("contrabass") {
        43
    } else if s.contains("bass guitar") || s.contains("electric bass") {
        34
    } else if s.contains("electric guitar") {
        27
    } else if s.contains("guitar") {
        24
    }
    // 3. Specific Winds
    else if s.contains("soprano sax") {
        64
    } else if s.contains("alto sax") {
        65
    } else if s.contains("tenor sax") {
        66
    } else if s.contains("baritone sax") {
        67
    } else if s.contains("trumpet") {
        56
    } else if s.contains("trombone") || s.contains("euphonium") {
        57
    } else if s.contains("tuba") {
        58
    } else if s.contains("horn") {
        60
    } else if s.contains("oboe") {
        68
    } else if s.contains("bassoon") {
        70
    } else if s.contains("clarinet") {
        71
    } else if s.contains("piccolo") {
        72
    } else if s.contains("flute") {
        73
    } else if s.contains("recorder") {
        74
    }
    // 4. Keyboard/Other
    else if s.contains("piano") {
        0
    } else if s.contains("synth") {
        2
    } else if s.contains("pizzicato") {
        45
    }
    // 5. Vocals (Generic terms like 'bass' can be vocal or instrument, so keep late)
    else if s.contains("soprano")
        || s.contains("alto")
        || s.contains("tenor")
        || s.contains("bass")
        || s.contains("baritone")
        || s.contains("vocal")
        || s.contains("voice")
    {
        52
    } else {
        -1
    }
}
