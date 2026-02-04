//! Unit tests for order CSV parsing.

use super::*;

mod parse_order_items_tests {
    use super::*;

    #[test]
    fn parses_single_item() {
        let items = parse_order_items("1x Card Name - 1,87 EUR", "12345", "Card Name").unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].product_id, "12345");
        assert_eq!(items[0].localized_product_name, "Card Name");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_multiple_items() {
        let items = parse_order_items(
            "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
            "111 | 222",
            "Card One | Card Two",
        )
        .unwrap();

        assert_eq!(items.len(), 2);

        assert_eq!(items[0].product_id, "111");
        assert_eq!(items[0].localized_product_name, "Card One");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 1.50).abs() < 0.001);

        assert_eq!(items[1].product_id, "222");
        assert_eq!(items[1].localized_product_name, "Card Two");
        assert_eq!(items[1].quantity, 2);
        assert!((items[1].price - 3.00).abs() < 0.001);
    }

    #[test]
    fn handles_mismatched_counts_as_single() {
        // When counts don't match, treat as single item
        let items = parse_order_items(
            "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
            "111", // Only one ID
            "Card One | Card Two",
        )
        .unwrap();

        assert_eq!(items.len(), 1);
    }

    #[test]
    fn handles_pipe_in_card_name() {
        // Card names can contain " | " (e.g., "Magic: The Gathering | Marvel's Spider-Man")
        // The parser should use product ID count as authoritative
        let items = parse_order_items(
            "1x Moss Diamond - 0,02 EUR | 1x Robot Token (Magic: The Gathering | Marvel's Spider-Man) - 0,02 EUR",
            "512140 | 848235",
            "Moss Diamond | Robot Token",
        )
        .unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].product_id, "512140");
        assert_eq!(items[0].localized_product_name, "Moss Diamond");
        assert!((items[0].price - 0.02).abs() < 0.001);

        assert_eq!(items[1].product_id, "848235");
        assert_eq!(items[1].localized_product_name, "Robot Token");
        assert!(items[1].description.contains("Marvel's Spider-Man"));
        assert!((items[1].price - 0.02).abs() < 0.001);
    }

    #[test]
    fn handles_real_cardmarket_format() {
        // Real Cardmarket format: "1x Card (Set) - CollectorNum - Rarity - Condition - Language - Price EUR"
        let items = parse_order_items(
            "1x Moss Diamond (Commander Legends) - 327 - Common - NM - English - 0,02 EUR | 2x Gift of Paradise (Commander Legends) - 229 - Common - NM - English - 0,04 EUR",
            "512140 | 510645",
            "Moss Diamond | Gift of Paradise",
        )
        .unwrap();

        assert_eq!(items.len(), 2);

        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 0.02).abs() < 0.001);

        assert_eq!(items[1].quantity, 2);
        assert!((items[1].price - 0.04).abs() < 0.001);
    }

    #[test]
    fn handles_large_real_order_with_44_items() {
        // Real order from Cardmarket export with 44 articles, including:
        // - Cards with embedded " | " in names (Marvel's Spider-Man crossover)
        // - Multiple quantities (2x, 4x)
        // - Various set names and conditions
        let description = "1x Moss Diamond (Commander Legends) - 327 - Common - NM - English - 0,02 EUR | 1x Secluded Courtyard (Magic: The Gathering Foundations) - 267 - Uncommon - NM - English - 0,34 EUR | 1x Fairgrounds Warden (Kaladesh) - 13 - Uncommon - LP - English - 0,02 EUR | 1x Ilysian Caryatid (Theros Beyond Death) - 174 - Common - EX - English - 0,15 EUR | 1x Cinderclasm (Zendikar Rising) - 136 - Uncommon - EX - English - 0,14 EUR | 1x Unclaimed Territory (Commander: The Lost Caverns of Ixalan) - 366 - Uncommon - EX - English - 0,46 EUR | 1x Secluded Steppe (Commander Legends: Extras) - 491 - Uncommon - EX - English - 0,14 EUR | 1x Kessig Wolf Run (Commander: The Lost Caverns of Ixalan) - 340 - Rare - NM - English - 0,30 EUR | 1x Cultivate (Commander: The Lord of the Rings: Tales of Middle-earth) - 236 - Common - EX - English - 0,70 EUR | 1x Selvala's Stampede (Conspiracy: Take the Crown) - 71 - Rare - GD - English - 2,21 EUR | 1x Setessan Oathsworn (Born of the Gods) - 138 - Common - LP - English - 0,02 EUR | 1x Robot Token (A 1/1) // Food Token (Magic: The Gathering | Marvel's Spider-Man: Tokens) - T 6/5 - Token - NM - English - 0,02 EUR | 1x Wayward Swordtooth (Rivals of Ixalan) - 150 - Rare - EX - English - 2,38 EUR | 1x Ancestral Blade (Commander Legends) - 5 - Common - NM - English - 0,02 EUR | 1x Raise the Alarm (Commander Legends) - 41 - Common - NM - English - 0,02 EUR | 1x Inspiring Roar (Commander Legends) - 23 - Common - NM - English - 0,02 EUR | 1x Wild Celebrants (Commander Legends) - 212 - Common - NM - English - 0,02 EUR | 1x Court Street Denizen (Commander Legends) - 17 - Common - NM - English - 0,02 EUR | 1x Lys Alana Bowmaster (Commander Legends) - 241 - Common - NM - English - 0,02 EUR | 1x Jalum Tome (Commander Legends) - 318 - Common - NM - English - 0,02 EUR | 1x Fin-Clade Fugitives (Commander Legends) - 227 - Common - NM - English - 0,02 EUR | 1x Lifecrafter's Gift (Commander Legends) - 240 - Common - NM - English - 0,02 EUR | 1x Spark Harvest (Commander Legends) - 150 - Common - NM - English - 0,02 EUR | 2x Gift of Paradise (Commander Legends) - 229 - Common - NM - English - 0,02 EUR | 1x Wildsize (Commander Legends) - 264 - Common - NM - English - 0,02 EUR | 1x Iona's Judgment (Commander Legends) - 25 - Common - NM - English - 0,02 EUR | 1x Dispeller's Capsule (Commander Legends) - 18 - Common - NM - English - 0,02 EUR | 1x Kinsbaile Courier (Commander Legends) - 29 - Common - NM - English - 0,02 EUR | 1x Seer's Lantern (Commander Legends) - 338 - Common - NM - English - 0,02 EUR | 4x Opal Palace (Commander Legends) - 352 - Common - NM - English - 0,02 EUR | 1x Palace Sentinels (Commander Legends) - 36 - Common - NM - English - 0,02 EUR | 1x Ninth Bridge Patrol (Commander Legends) - 33 - Common - NM - English - 0,02 EUR | 1x Captain's Call (Commander Legends) - 15 - Common - NM - English - 0,02 EUR | 1x Ripscale Predator (Commander Legends) - 196 - Common - NM - English - 0,02 EUR | 1x Skywhaler's Shot (Commander Legends) - 46 - Common - NM - English - 0,02 EUR | 1x Welding Sparks (Commander Legends) - 210 - Common - NM - English - 0,02 EUR | 4x Rupture Spire (Commander Legends) - 355 - Common - NM - English - 0,02 EUR";

        let product_ids = "512140 | 797300 | 292765 | 432364 | 496050 | 743717 | 514944 | 743692 | 717430 | 291908 | 266057 | 848235 | 315452 | 510610 | 510145 | 510960 | 511210 | 511035 | 514140 | 514260 | 514125 | 514135 | 512115 | 510645 | 514190 | 513845 | 513835 | 510930 | 514290 | 513805 | 513735 | 510510 | 510620 | 514080 | 513870 | 512025 | 514325";

        let product_names = "Moss Diamond | Secluded Courtyard | Fairgrounds Warden | Ilysian Caryatid | Cinderclasm | Unclaimed Territory | Secluded Steppe | Kessig Wolf Run | Cultivate | Selvala's Stampede | Setessan Oathsworn | Robot Token (A 1/1) // Food Token | Wayward Swordtooth | Ancestral Blade | Raise the Alarm | Inspiring Roar | Wild Celebrants | Court Street Denizen | Lys Alana Bowmaster | Jalum Tome | Fin-Clade Fugitives | Lifecrafter's Gift | Spark Harvest | Gift of Paradise | Wildsize | Iona's Judgment | Dispeller's Capsule | Kinsbaile Courier | Seer's Lantern | Opal Palace | Palace Sentinels | Ninth Bridge Patrol | Captain's Call | Ripscale Predator | Skywhaler's Shot | Welding Sparks | Rupture Spire";

        let items = parse_order_items(description, product_ids, product_names).unwrap();

        // Should parse all 37 distinct line items (some have quantities > 1)
        assert_eq!(items.len(), 37);

        // Verify first item
        assert_eq!(items[0].product_id, "512140");
        assert_eq!(items[0].localized_product_name, "Moss Diamond");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 0.02).abs() < 0.001);

        // Verify item with embedded pipe (Robot Token from Marvel's Spider-Man)
        // This is item index 11
        assert_eq!(items[11].product_id, "848235");
        assert!(items[11]
            .description
            .contains("Marvel's Spider-Man: Tokens"));
        assert_eq!(items[11].quantity, 1);

        // Verify multi-quantity items
        // Gift of Paradise (2x) at index 23
        assert_eq!(items[23].localized_product_name, "Gift of Paradise");
        assert_eq!(items[23].quantity, 2);

        // Opal Palace (4x) at index 29
        assert_eq!(items[29].localized_product_name, "Opal Palace");
        assert_eq!(items[29].quantity, 4);

        // Rupture Spire (4x) at index 36 (last item)
        assert_eq!(items[36].localized_product_name, "Rupture Spire");
        assert_eq!(items[36].quantity, 4);

        // Verify merchandise value: sum of (quantity × per-unit price) must equal CSV total
        // Description prices are per-unit, so 4x at 0.02 EUR means 0.08 EUR line total
        let merchandise_value: f64 = items.iter().map(|i| i.quantity as f64 * i.price).sum();
        assert!(
            (merchandise_value - 7.52).abs() < 0.01,
            "Merchandise value {merchandise_value:.2} should match CSV value 7.52 EUR"
        );

        // Also verify total article count: sum of all quantities = 44 articles
        let total_articles: u32 = items.iter().map(|i| i.quantity).sum();
        assert_eq!(
            total_articles, 44,
            "Total articles should be 44 (as stated in test name)"
        );
    }

    #[test]
    fn handles_massive_order_with_211_articles_and_embedded_pipes() {
        // Real order from Cardmarket with 211 articles (128 line items), including:
        // - Multiple Avatar: The Last Airbender crossover cards with " | " in names
        // - Marvel's Spider-Man crossover cards with " | " in names
        // - FINAL FANTASY crossover cards
        // - Many quantities (2x, 3x, 4x, 5x, 6x)
        // - Various conditions (NM, EX, GD, LP) and languages (English, German)
        let description = "2x Mishra's Factory (V.1) (Modern Horizons 2) - 302 - Uncommon - NM - English - 0,14 EUR | 1x Runeflare Trap (Zendikar) - 146 - Uncommon - GD - English - 0,16 EUR | 1x Edge of the Divinity (Eventide) - 87 - Common - LP - German - 0,14 EUR | 3x Mishra's Factory (Masters 25) - 242 - Uncommon - GD - English - 0,16 EUR | 1x Lightning Helix (V.1) (Mystical Archive) - 62 - Rare - EX - English - 0,67 EUR | 1x Qasali Ambusher (Shards of Alara) - 184 - Uncommon - LP - English - 0,50 EUR | 1x Mage Slayer (Planechase) - Uncommon - GD - English - 1,23 EUR | 2x Hidden Footblade (Universes Beyond: Assassin's Creed) - 34 - Uncommon - NM - English - 0,18 EUR | 1x Sinkhole Surveyor (Tarkir: Dragonstorm) - 93 - Rare - NM - English - Foil - 0,32 EUR | 1x The Sibsig Ceremony (Tarkir: Dragonstorm) - 91 - Rare - NM - English - 0,28 EUR | 1x Lasyd Prowler (Tarkir: Dragonstorm) - 149 - Rare - NM - English - 0,19 EUR | 1x Veteran Ice Climber (Tarkir: Dragonstorm) - 64 - Uncommon - NM - English - 0,14 EUR | 2x Molt Tender (Aetherdrift) - 171 - Uncommon - NM - English - 0,28 EUR | 1x Terrian, World Tyrant (Aetherdrift) - 182 - Uncommon - NM - English - 0,15 EUR | 1x Greasewrench Goblin (Aetherdrift) - 132 - Uncommon - NM - English - 0,14 EUR | 1x Transit Mage (Aetherdrift) - 70 - Uncommon - NM - English - 0,23 EUR | 1x Shocking Sharpshooter (Tarkir: Dragonstorm) - 121 - Uncommon - NM - English - 0,17 EUR | 1x Gas Guzzler (Aetherdrift: Extras) - 338 - Rare - NM - English - 0,28 EUR | 1x Voyager Glidecar (Aetherdrift) - 36 - Rare - NM - English - 0,19 EUR | 1x Gas Guzzler (Aetherdrift) - 85 - Rare - NM - English - 0,19 EUR | 1x Regal Imperiosaur (Aetherdrift) - 177 - Rare - NM - English - 0,39 EUR | 1x Webstrike Elite (Aetherdrift) - 186 - Rare - NM - English - 0,21 EUR | 1x Zahur, Glory's Past (Aetherdrift) - 229 - Rare - NM - English - 0,25 EUR | 1x Anthem of Champions (Magic: The Gathering Foundations) - 116 - Rare - NM - English - 0,21 EUR | 1x Sylvan Scavenging (Magic: The Gathering Foundations) - 113 - Rare - NM - English - 0,22 EUR | 1x Ajani, Caller of the Pride (Magic: The Gathering Foundations) - 134 - Mythic - NM - English - 0,91 EUR | 1x Demon of Catastrophes (Core 2019) - 91 - Rare - EX - English - 0,22 EUR | 1x Diamond Lion (Modern Horizons 2) - 225 - Rare - EX - English - 0,19 EUR | 1x Lupinflower Village (Bloomburrow) - 256 - Uncommon - NM - English - 0,14 EUR | 2x Heartfire Hero (Bloomburrow) - 138 - Uncommon - NM - English - 0,67 EUR | 1x Shrike Force (Bloomburrow) - 31 - Uncommon - NM - English - 0,21 EUR | 4x Sunshower Druid (Bloomburrow) - 195 - Common - NM - English - 0,05 EUR | 1x Duskwatch Recruiter / Krallenhorde Howler (V.1) (Innistrad Remastered: Extras) - 323 - Uncommon - NM - English - 0,39 EUR | 2x Abundant Growth (Innistrad Remastered) - 184 - Common - NM - English - 0,15 EUR | 1x Young Wolf (Innistrad Remastered) - 227 - Common - NM - English - 0,15 EUR | 1x Twinblade Geist // Twinblade Invocation (Innistrad Remastered) - 47 - Uncommon - NM - English - 0,14 EUR | 1x Ghoultree (Innistrad Remastered) - 198 - Uncommon - NM - English - 0,20 EUR | 1x Lupine Prototype (Innistrad Remastered) - 267 - Uncommon - NM - English - 0,14 EUR | 2x Decimator of the Provinces (Innistrad Remastered) - 2 - Rare - NM - English - 0,36 EUR | 2x Glint-Nest Crane (Kaladesh) - 50 - Uncommon - EX - English - 0,15 EUR | 2x Morbid Curiosity (Kaladesh) - 94 - Uncommon - EX - English - 0,14 EUR | 1x Skirsdag High Priest (Innistrad Remastered) - 132 - Rare - NM - English - 0,19 EUR | 1x Bedlam Reveler (Innistrad Remastered) - 142 - Rare - NM - English - 0,22 EUR | 1x Kruin Outlaw / Terror of Kruin Pass (Innistrad Remastered) - 161 - Rare - NM - English - 0,26 EUR | 1x Collective Brutality (Innistrad Remastered) - 101 - Rare - NM - English - 0,46 EUR | 1x Rabbit Battery (Kamigawa: Neon Dynasty) - 157 - Uncommon - EX - English - 0,21 EUR | 1x Twinblade Geist // Twinblade Invocation (Innistrad: Crimson Vow) - 40 - Uncommon - EX - English - 0,15 EUR | 2x Ascendant Packleader (Innistrad: Crimson Vow) - 186 - Rare - EX - English - 0,32 EUR | 1x Sticky Fingers (Streets of New Capenna) - 124 - Common - EX - English - 0,21 EUR | 1x Sorin, Vengeful Bloodlord (War of the Spark) - 217 - Rare - EX - German - 1,14 EUR | 1x Mosswood Dreadknight // Dread Whispers (Wilds of Eldraine) - 231 - Rare - EX - English - 0,76 EUR | 1x Realm-Scorcher Hellkite (Wilds of Eldraine) - 145 - Mythic - EX - English - 0,81 EUR | 1x Callous Sell-Sword // Burn Together (Wilds of Eldraine) - 221 - Uncommon - EX - English - 0,20 EUR | 1x Egon, God of Death // Throne of Death (Kaldheim: Extras) - 306 - Rare - EX - English - 0,41 EUR | 3x Silhana Ledgewalker (Ravnica Remastered) - 156 - Common - EX - English - 0,10 EUR | 1x Stalking Vengeance (Ravnica Remastered) - 126 - Uncommon - EX - English - 0,14 EUR | 4x Judge's Familiar (Ravnica Remastered) - 192 - Common - EX - English - 0,18 EUR | 2x Mask of Memory (Commander: Phyrexia: All Will Be One) - 136 - Uncommon - EX - English - 0,25 EUR | 1x Veteran Beastrider (Aetherdrift) - 226 - Uncommon - EX - English - 0,14 EUR | 1x Light Up the Stage (Magic: The Gathering - FINAL FANTASY Through the Ages) - 39 - Uncommon - NM - English - 0,19 EUR | 1x Ghalta, Primal Hunger (Foundations Jumpstart) - 77 - Rare - NM - English - 0,62 EUR | 3x Scout for Survivors (Edge of Eternities) - 33 - Uncommon - NM - English - 0,14 EUR | 2x Xu-Ifit, Osteoharmonist (Edge of Eternities) - 127 - Rare - NM - English - 0,31 EUR | 1x Volcano Hellion (Planar Chaos) - 111 - Rare - EX - English - 0,35 EUR | 1x Lupine Prototype (Eldritch Moon) - 197 - Rare - GD - English - 0,21 EUR | 1x Sinkhole Surveyor (Tarkir: Dragonstorm: Extras) - 342 - Rare - EX - English - 0,24 EUR | 1x Lasyd Prowler (Tarkir: Dragonstorm) - 149 - Rare - EX - English - 0,19 EUR | 1x Come Back Wrong (Duskmourn: House of Horror) - 86 - Rare - EX - English - 0,35 EUR | 1x Niko, Light of Hope (Duskmourn: House of Horror) - 224 - Mythic - EX - English - 0,33 EUR | 1x Doomsday Excruciator (Duskmourn: House of Horror) - 94 - Rare - EX - English - 0,30 EUR | 1x Wall of Reverence (Commander: Tarkir: Dragonstorm) - 139 - Rare - EX - English - 0,28 EUR | 1x Salvage Titan (Double Masters) - 104 - Rare - EX - English - 0,24 EUR | 1x Disciple of Bolas (Magic 2013) - 88 - Rare - EX - English - 0,55 EUR | 1x Leyline Tyrant (Zendikar Rising) - 147 - Mythic - EX - English - 1,35 EUR | 1x Magus of the Candelabra (Commander: Modern Horizons 3) - 236 - Rare - NM - English - 0,23 EUR | 1x Irresistible Prey (Conspiracy: Take the Crown) - 183 - Uncommon - GD - English - 0,14 EUR | 4x Flame Slash (Conspiracy: Take the Crown) - 157 - Common - GD - English - 0,36 EUR | 4x Faithless Looting (Dark Ascension) - 87 - Common - EX - English - 0,47 EUR | 4x Myr Superion (New Phyrexia) - 146 - Rare - EX - English - 1,85 EUR | 1x Nissa, Voice of Zendikar (Oath of the Gatewatch) - 138 - Mythic - EX - English - 1,53 EUR | 1x Thundermaw Hellkite (Magic 2013) - 150 - Mythic - EX - English - 1,90 EUR | 4x Leatherback Baloth (Worldwake) - 107 - Uncommon - EX - English - 0,30 EUR | 1x Chandra, Pyromaster (Magic 2015) - 134 - Mythic - EX - English - 0,69 EUR | 3x Heartless Summoning (Innistrad) - 104 - Rare - EX - English - 0,94 EUR | 3x Pelt Collector (Guilds of Ravnica) - 141 - Rare - EX - English - 0,62 EUR | 3x Yorvo, Lord of Garenbrig (Throne of Eldraine) - 185 - Rare - EX - English - 0,24 EUR | 2x Serra Avenger (Magic 2013) - 33 - Rare - EX - English - 0,39 EUR | 1x Hazoret the Fervent (Amonkhet) - 136 - Mythic - EX - English - 0,72 EUR | 4x Talara's Battalion (Eventide) - 77 - Rare - EX - English - 0,68 EUR | 4x Traverse the Ulvenwald (Shadows over Innistrad) - 234 - Rare - EX - English - 0,54 EUR | 1x Kaya's Guile (Modern Horizons) - 205 - Rare - EX - English - 1,02 EUR | 3x Stormbreath Dragon (Theros) - 143 - Mythic - EX - English - 0,79 EUR | 4x Infernal Tutor (Dissension) - 46 - Rare - EX - English - 1,03 EUR | 4x Hollow One (Hour of Devastation) - 163 - Rare - EX - English - 1,02 EUR | 1x Fracturing Gust (Shadowmoor) - 227 - Rare - EX - English - 1,35 EUR | 4x Leyline of Sanctity (Modern Masters 2015) - 23 - Rare - EX - English - 1,09 EUR | 1x Kolaghan's Command (Commander: Modern Horizons 3) - 268 - Rare - EX - English - 0,39 EUR | 1x Liliana, Death's Majesty (Commander: Modern Horizons 3) - 200 - Mythic - EX - English - 0,40 EUR | 4x Dungrove Elder (Magic 2012) - 171 - Rare - EX - English - 0,81 EUR | 3x Collective Brutality (Eldritch Moon) - 85 - Rare - EX - English - 0,52 EUR | 2x Lightning Skelemental (Modern Horizons) - 208 - Rare - EX - English - 0,83 EUR | 1x Darksteel Citadel (Darksteel) - 164 - Common - LP - German - 0,47 EUR | 1x Temur Battle Rage (Fate Reforged) - 116 - Common - EX - English - 0,20 EUR | 6x Flameblade Adept (Amonkhet) - 131 - Uncommon - EX - English - 0,22 EUR | 1x Shattering Spree (Guildpact) - 75 - Uncommon - GD - English - 0,69 EUR | 3x Ornithopter (Mirrodin) - 224 - Uncommon - LP - German - 0,22 EUR | 3x Nameless Inversion (Player Rewards Promos) - 04-09 - Rare - EX - English - 0,54 EUR | 3x Groundswell (Duel Decks: Zendikar vs. Eldrazi) - 15 - Common - EX - English - 0,59 EUR | 1x Grisly Salvage (Commander: Modern Horizons 3) - 263 - Common - EX - English - 0,17 EUR | 1x Spectral Procession (Modern Masters 2015) - 33 - Uncommon - EX - English - 0,24 EUR | 2x Mask of Memory (Mirrodin) - 203 - Uncommon - LP - English - 0,26 EUR | 4x Crack the Earth (Betrayers of Kamigawa) - 98 - Common - EX - English - 0,25 EUR | 1x Gnaw to the Bone (Innistrad) - 183 - Common - GD - English - 0,71 EUR | 2x Browbeat (Planechase) - Uncommon - EX - English - 0,39 EUR | 5x Apostle's Blessing (Modern Masters 2015) - 8 - Common - EX - English - 0,45 EUR | 1x Hallowed Burial (Conspiracy: Take the Crown) - 91 - Rare - EX - English - 0,52 EUR | 1x Emissary Escort (V.2) (Edge of Eternities: Promos) - 56 - Rare - NM - English - 0,19 EUR | 1x Bender's Waterskin (Magic: The Gathering | Avatar: The Last Airbender) - 255 - Common - NM - English - 0,35 EUR | 1x Tolls of War (Magic: The Gathering | Avatar: The Last Airbender) - 245 - Uncommon - NM - English - 0,14 EUR | 1x Shadow of the Goblin (Magic: The Gathering | Marvel's Spider-Man) - 87 - Rare - NM - English - 0,62 EUR | 1x Origin of Metalbending (Magic: The Gathering | Avatar: The Last Airbender) - 187 - Common - NM - English - 0,14 EUR | 1x Octopus Form (Magic: The Gathering | Avatar: The Last Airbender) - 66 - Common - NM - English - 0,11 EUR | 1x Night's Whisper (Commander: Edge of Eternities) - 85 - Common - EX - English - 0,78 EUR | 1x Mass Hysteria (Innistrad Remastered: Extras) - 400 - Rare - EX - English - 1,07 EUR | 2x Curious Farm Animals (Magic: The Gathering | Avatar: The Last Airbender) - 14 - Common - NM - English - 0,09 EUR";

        let product_ids = "566256 | 21935 | 19549 | 319042 | 556924 | 19725 | 21540 | 775448 | 819005 | 817948 | 818646 | 819382 | 807968 | 808817 | 807959 | 808410 | 819001 | 808464 | 809494 | 808411 | 808815 | 809075 | 807212 | 795115 | 797270 | 795117 | 359685 | 564640 | 776461 | 777769 | 777553 | 776459 | 805907 | 805811 | 802815 | 805710 | 804995 | 805869 | 805674 | 292665 | 292512 | 805772 | 805778 | 805792 | 805749 | 608244 | 581693 | 581928 | 652187 | 371906 | 730026 | 728064 | 728415 | 530867 | 748509 | 748477 | 748545 | 693311 | 808426 | 826329 | 795498 | 834644 | 836323 | 14290 | 290977 | 819273 | 818646 | 786366 | 786607 | 786442 | 818753 | 484454 | 256790 | 496154 | 772994 | 291916 | 291886 | 252468 | 245969 | 287049 | 256793 | 22149 | 267511 | 250647 | 364299 | 401309 | 256787 | 296694 | 19539 | 288995 | 375117 | 264074 | 13018 | 298721 | 19241 | 282809 | 773214 | 773186 | 247926 | 290982 | 374613 | 416 | 271697 | 296688 | 13252 | 224 | 20801 | 284145 | 773211 | 282891 | 203 | 12803 | 250750 | 21535 | 283049 | 291759 | 837009 | 844441 | 857749 | 846650 | 857470 | 857431 | 834091 | 802777 | 858207";

        let product_names = "Mishra's Factory (V.1) | Runeflare Trap | Zwiefalt der Gottheit | Mishra's Factory | Lightning Helix (V.1) | Qasali Ambusher | Mage Slayer | Hidden Footblade | Sinkhole Surveyor | The Sibsig Ceremony | Lasyd Prowler | Veteran Ice Climber | Molt Tender | Terrian, World Tyrant | Greasewrench Goblin | Transit Mage | Shocking Sharpshooter | Gas Guzzler | Voyager Glidecar | Gas Guzzler | Regal Imperiosaur | Webstrike Elite | Zahur, Glory's Past | Anthem of Champions | Sylvan Scavenging | Ajani, Caller of the Pride | Demon of Catastrophes | Diamond Lion | Lupinflower Village | Heartfire Hero | Shrike Force | Sunshower Druid | Duskwatch Recruiter / Krallenhorde Howler (V.1) | Abundant Growth | Young Wolf | Twinblade Geist // Twinblade Invocation | Ghoultree | Lupine Prototype | Decimator of the Provinces | Glint-Nest Crane | Morbid Curiosity | Skirsdag High Priest | Bedlam Reveler | Kruin Outlaw / Terror of Kruin Pass | Collective Brutality | Rabbit Battery | Twinblade Geist // Twinblade Invocation | Ascendant Packleader | Sticky Fingers | Sorin, rachsüchtiger Blutfürst | Mosswood Dreadknight // Dread Whispers | Realm-Scorcher Hellkite | Callous Sell-Sword // Burn Together | Egon, God of Death // Throne of Death | Silhana Ledgewalker | Stalking Vengeance | Judge's Familiar | Mask of Memory | Veteran Beastrider | Light Up the Stage | Ghalta, Primal Hunger | Scout for Survivors | Xu-Ifit, Osteoharmonist | Volcano Hellion | Lupine Prototype | Sinkhole Surveyor | Lasyd Prowler | Come Back Wrong | Niko, Light of Hope | Doomsday Excruciator | Wall of Reverence | Salvage Titan | Disciple of Bolas | Leyline Tyrant | Magus of the Candelabra | Irresistible Prey | Flame Slash | Faithless Looting | Myr Superion | Nissa, Voice of Zendikar | Thundermaw Hellkite | Leatherback Baloth | Chandra, Pyromaster | Heartless Summoning | Pelt Collector | Yorvo, Lord of Garenbrig | Serra Avenger | Hazoret the Fervent | Talara's Battalion | Traverse the Ulvenwald | Kaya's Guile | Stormbreath Dragon | Infernal Tutor | Hollow One | Fracturing Gust | Leyline of Sanctity | Kolaghan's Command | Liliana, Death's Majesty | Dungrove Elder | Collective Brutality | Lightning Skelemental | Nachtstahl-Zitadelle | Temur Battle Rage | Flameblade Adept | Shattering Spree | Ornithopter | Nameless Inversion | Groundswell | Grisly Salvage | Spectral Procession | Mask of Memory | Crack the Earth | Gnaw to the Bone | Browbeat | Apostle's Blessing | Hallowed Burial | Emissary Escort (V.2) | Bender's Waterskin | Tolls of War | Shadow of the Goblin | Origin of Metalbending | Octopus Form | Night's Whisper | Mass Hysteria | Curious Farm Animals";

        let items = parse_order_items(description, product_ids, product_names).unwrap();

        // Should parse all 125 line items
        assert_eq!(items.len(), 125);

        // Verify first item
        assert_eq!(items[0].product_id, "566256");
        assert_eq!(items[0].localized_product_name, "Mishra's Factory (V.1)");
        assert_eq!(items[0].quantity, 2);
        assert!((items[0].price - 0.14).abs() < 0.001);

        // Verify Avatar: The Last Airbender card with embedded pipe (Bender's Waterskin)
        // This should be near the end of the list
        let bender_item = items
            .iter()
            .find(|i| i.localized_product_name == "Bender's Waterskin")
            .expect("Should find Bender's Waterskin");
        assert!(bender_item
            .description
            .contains("Avatar: The Last Airbender"));
        assert_eq!(bender_item.quantity, 1);
        assert!((bender_item.price - 0.35).abs() < 0.001);

        // Verify Marvel's Spider-Man card with embedded pipe
        let spider_item = items
            .iter()
            .find(|i| i.localized_product_name == "Shadow of the Goblin")
            .expect("Should find Shadow of the Goblin");
        assert!(spider_item.description.contains("Marvel's Spider-Man"));
        assert_eq!(spider_item.quantity, 1);
        assert!((spider_item.price - 0.62).abs() < 0.001);

        // Verify last item (Curious Farm Animals with 2x quantity)
        assert_eq!(items[124].localized_product_name, "Curious Farm Animals");
        assert_eq!(items[124].quantity, 2);
        assert!((items[124].price - 0.09).abs() < 0.001);

        // Verify high-value items are correctly parsed
        let thundermaw = items
            .iter()
            .find(|i| i.localized_product_name == "Thundermaw Hellkite")
            .expect("Should find Thundermaw Hellkite");
        assert!((thundermaw.price - 1.90).abs() < 0.001);

        // Verify 6x quantity item (Flameblade Adept)
        let flameblade = items
            .iter()
            .find(|i| i.localized_product_name == "Flameblade Adept")
            .expect("Should find Flameblade Adept");
        assert_eq!(flameblade.quantity, 6);

        // Verify merchandise value: sum of (quantity × per-unit price) must equal CSV total
        // Description prices are per-unit, so 2x at 0.14 EUR means 0.28 EUR line total
        let merchandise_value: f64 = items.iter().map(|i| i.quantity as f64 * i.price).sum();
        assert!(
            (merchandise_value - 96.71).abs() < 0.01,
            "Merchandise value {merchandise_value:.2} should match CSV value 96.71 EUR"
        );

        // Also verify total article count: sum of all quantities = 211 articles
        let total_articles: u32 = items.iter().map(|i| i.quantity).sum();
        assert_eq!(
            total_articles, 211,
            "Total articles should be 211 (as stated in test name)"
        );
    }
}

mod split_descriptions_by_count_tests {
    use super::*;

    #[test]
    fn simple_split_matches_count() {
        let result = split_descriptions_by_count("1x Card A - 1,00 EUR | 1x Card B - 2,00 EUR", 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1x Card A - 1,00 EUR");
        assert_eq!(result[1], "1x Card B - 2,00 EUR");
    }

    #[test]
    fn handles_embedded_pipe_in_card_name() {
        let desc = "1x Card A - 1,00 EUR | 1x Token (Set | Subset) - 2,00 EUR";
        let result = split_descriptions_by_count(desc, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1x Card A - 1,00 EUR");
        assert!(result[1].contains("Set | Subset"));
    }

    #[test]
    fn returns_single_for_count_one() {
        let result = split_descriptions_by_count("1x Card - 1,00 EUR", 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "1x Card - 1,00 EUR");
    }

    #[test]
    fn handles_multiple_pipes_in_description() {
        // Two items, but description has 3 pipe separators due to embedded pipe
        let desc = "1x Card A - 1,00 EUR | 1x Token (A | B | C) - 2,00 EUR";
        let result = split_descriptions_by_count(desc, 2);
        assert_eq!(result.len(), 2);
        assert!(result[1].contains("A | B | C"));
    }
}

mod parse_order_line_tests {
    use super::*;

    #[test]
    fn parses_valid_order_line() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

        assert_eq!(order.order_id, "1234567");
        assert_eq!(order.username, "user123");
        assert_eq!(order.name, "John Doe");
        assert_eq!(order.street, "Main Street 1");
        assert_eq!(order.zip, "10557");
        assert_eq!(order.city, "Berlin");
        assert_eq!(order.country, "Germany");
        assert_eq!(order.date_of_purchase, "2025-01-15");
        assert_eq!(order.article_count, 1);
        assert_eq!(order.merchandise_value, "5,00");
        assert_eq!(order.shipment_costs, "1,50");
        assert_eq!(order.total_value, "6,50");
        assert_eq!(order.currency, "EUR");
        assert_eq!(order.product_id, "98765");
        assert_eq!(order.localized_product_name, "Card Name");
    }

    #[test]
    fn parses_order_with_optional_fields_empty() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

        assert!(order.is_professional.is_none());
        assert!(order.vat_number.is_none());
    }

    #[test]
    fn parses_order_with_professional_flag() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;yes;DE123456789;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

        assert_eq!(order.is_professional, Some("yes".to_string()));
        assert_eq!(order.vat_number, Some("DE123456789".to_string()));
    }

    #[test]
    fn fails_with_insufficient_columns() {
        let line = "1234567;user123;John Doe";

        let result = parse_order_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn fails_with_invalid_article_count() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;not_a_number;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let result = parse_order_line(line);
        assert!(result.is_err());
    }
}

mod parse_csv_with_headers_tests {
    use super::*;

    #[test]
    fn parses_csv_with_headers() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let orders = parse_csv_with_headers(content).unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "1234567");
        assert_eq!(orders[0].name, "John Doe");
    }

    #[test]
    fn parses_multiple_orders() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user1;John Doe;Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card One\n\
                      1234568;user2;Jane Doe;Street 2;20095 Hamburg;Germany;;;2025-01-16;2;10,00;1,50;11,50;0,20;EUR;2x Card - 5,00 EUR;98766;Card Two";

        let orders = parse_csv_with_headers(content).unwrap();

        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].name, "John Doe");
        assert_eq!(orders[1].name, "Jane Doe");
    }

    #[test]
    fn returns_empty_for_empty_content() {
        // Empty content has no header, so parse_csv_with_headers won't be called
        // But if it is, it should return empty
        let orders = parse_csv_with_headers("").unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn returns_empty_for_header_only() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName";

        let orders = parse_csv_with_headers(content).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn skips_empty_lines() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      \n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name\n\
                      ";

        let orders = parse_csv_with_headers(content).unwrap();
        assert_eq!(orders.len(), 1);
    }
}
