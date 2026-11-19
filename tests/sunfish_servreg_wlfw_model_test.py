import unittest

from platforms.sunfish.servreg_wlfw_model import (
    DiscoveryError,
    DomainRecord,
    PdMapperModel,
    QrtrNameServiceModel,
    QrtrServiceKey,
    ROOT_PD,
    WLAN_PD,
    WLFW_KEY,
)


class SunfishServregWlfwModelTests(unittest.TestCase):
    def published_mapper(self):
        name_service = QrtrNameServiceModel()
        mapper = PdMapperModel()
        with self.assertRaisesRegex(DiscoveryError, "not ready"):
            mapper.publish(qrtr_name_service_ready=name_service.ready)
        name_service.start()
        mapper.publish(qrtr_name_service_ready=name_service.ready)
        return name_service, mapper

    def test_servreg_query_aggregates_both_captured_tms_domains(self):
        _name_service, mapper = self.published_mapper()

        result = set(mapper.get_domain_list("tms/servreg"))

        self.assertEqual(
            result,
            {
                DomainRecord(ROOT_PD, 180),
                DomainRecord(WLAN_PD, 180),
            },
        )

    def test_pdr_enabled_query_has_no_domain_in_captured_sunfish_manifests(self):
        _name_service, mapper = self.published_mapper()

        self.assertEqual(mapper.get_domain_list("tms/pdr_enabled"), ())

    def test_manifest_names_are_not_wlfw_qrtr_server_records(self):
        name_service, mapper = self.published_mapper()
        self.assertEqual(
            mapper.get_domain_list("wlan/fw"),
            (DomainRecord(WLAN_PD, 180),),
        )
        self.assertEqual(mapper.get_domain_list("wlfw"), ())

        self.assertIsNone(name_service.register_lookup(WLFW_KEY))
        self.assertIsNone(name_service.server_for_lookup(WLFW_KEY))

    def test_wlfw_lookup_requires_an_independent_server_advertisement(self):
        name_service = QrtrNameServiceModel()
        with self.assertRaisesRegex(DiscoveryError, "not ready"):
            name_service.register_lookup(WLFW_KEY)
        name_service.start()

        # A client lookup expresses interest; it does not start or publish WLFW.
        self.assertIsNone(name_service.register_lookup(WLFW_KEY))
        notices = name_service.advertise_server(
            WLFW_KEY,
            node=0,
            port=0x1234,
            source_domain=WLAN_PD,
        )

        self.assertEqual(len(notices), 1)
        self.assertEqual(notices[0].key, WLFW_KEY)
        self.assertEqual(notices[0].endpoint.node, 0)
        self.assertEqual(notices[0].endpoint.port, 0x1234)
        self.assertEqual(name_service.server_for_lookup(WLFW_KEY), notices[0].endpoint)

    def test_wlfw_cannot_be_attributed_to_root_pd_or_a_different_instance(self):
        name_service = QrtrNameServiceModel()
        name_service.start()
        name_service.register_lookup(WLFW_KEY)
        with self.assertRaisesRegex(DiscoveryError, "WLAN domain"):
            name_service.advertise_server(
                WLFW_KEY,
                node=0,
                port=0x1234,
                source_domain=ROOT_PD,
            )
        wrong_instance = QrtrServiceKey(WLFW_KEY.service_id, WLFW_KEY.version, 1)
        self.assertEqual(
            name_service.advertise_server(
                wrong_instance,
                node=0,
                port=0x2345,
                source_domain=WLAN_PD,
            ),
            (),
        )
        self.assertIsNone(name_service.server_for_lookup(WLFW_KEY))

    def test_mapper_cannot_answer_before_publication_or_without_maps(self):
        mapper = PdMapperModel()
        with self.assertRaisesRegex(DiscoveryError, "before publication"):
            mapper.get_domain_list("tms/servreg")

        empty_mapper = PdMapperModel(())
        with self.assertRaisesRegex(DiscoveryError, "no protection-domain manifests"):
            empty_mapper.publish(qrtr_name_service_ready=True)
        self.assertFalse(empty_mapper.published)

    def test_invalid_mapping_and_endpoint_evidence_fail_closed(self):
        name_service = QrtrNameServiceModel()
        name_service.start()
        with self.assertRaisesRegex(DiscoveryError, "invalid QRTR endpoint"):
            name_service.advertise_server(
                WLFW_KEY,
                node=0,
                port=0,
                source_domain=WLAN_PD,
            )
        with self.assertRaisesRegex(DiscoveryError, "source domain is unknown"):
            name_service.advertise_server(
                WLFW_KEY,
                node=0,
                port=0x1234,
                source_domain="",
            )
        with self.assertRaisesRegex(ValueError, "service ID"):
            QrtrServiceKey(True, 1, 0)

    def test_duplicate_manifest_records_fail_closed(self):
        duplicate = (
            ("tms", "servreg", ROOT_PD, 180),
            ("tms", "servreg", ROOT_PD, 180),
        )
        with self.assertRaisesRegex(DiscoveryError, "duplicate manifest mapping"):
            PdMapperModel(duplicate)


if __name__ == "__main__":
    unittest.main()
