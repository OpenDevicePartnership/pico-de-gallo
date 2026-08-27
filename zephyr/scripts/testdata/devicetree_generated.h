/*
 * Fixture excerpt of a Zephyr-generated devicetree header.
 *
 * Only the shapes the ci-build.sh ordinal resolver depends on. Node paths and
 * the ordinal 49 follow the values recorded for spi_bridge in
 * docs/superpowers/specs/2026-08-19-zephyr-mfd-m4-acceptance.md check A-11.
 *
 * The _ORD_STR_SORTABLE lines are load-bearing: a resolver that matches them
 * returns the wrong define. Do not delete them.
 */

#define DT_N_S_pico_de_gallo_S_spi_ORD 12
#define DT_N_S_pico_de_gallo_S_spi_ORD_STR_SORTABLE "00012"

#define DT_N_S_pico_de_gallo_S_spi_S_is31fl3743b_0_ORD 49
#define DT_N_S_pico_de_gallo_S_spi_S_is31fl3743b_0_ORD_STR_SORTABLE "00049"

#define DT_N_S_pico_de_gallo_S_i2c_S_tmp117_48_ORD 50
#define DT_N_S_pico_de_gallo_S_i2c_S_tmp117_48_ORD_STR_SORTABLE "00050"
