import unittest

from platforms.sunfish.modem_lifecycle_model import (
    ModemLifecycleError,
    ModemLifecycleModel,
    ModemState,
)


class ModemLifecycleModelTests(unittest.TestCase):
    def prepared(self):
        model = ModemLifecycleModel()
        model.prepare_boot(
            firmware_bundle_identified=True,
            image_layout_valid=True,
            memory_excluded=True,
            scm_operation_allowlisted=True,
            ipa_host_ready=True,
            servreg_mapper_ready=True,
            remote_storage_ready=True,
            heap_memory_ready=True,
            tftp_provider_ready=True,
        )
        return model

    def authenticated(self):
        model = self.prepared()
        model.power_sequence_complete(aop_sequence_verified=True)
        model.secure_boot_complete(pas_authentication_succeeded=True)
        return model

    def peer_ready(self):
        model = self.authenticated()
        peer = model.modem_ready(
            after_powerup_verified=True,
            fresh_reset_verified=True,
            exclusive_transport_verified=True,
        )
        return model, peer

    def test_boot_requires_all_preflight_evidence(self):
        model = ModemLifecycleModel()
        with self.assertRaisesRegex(ModemLifecycleError, "unverified"):
            model.prepare_boot(
                firmware_bundle_identified=True,
                image_layout_valid=True,
                memory_excluded=False,
                scm_operation_allowlisted=True,
                ipa_host_ready=True,
                servreg_mapper_ready=True,
                remote_storage_ready=True,
                heap_memory_ready=True,
                tftp_provider_ready=True,
            )
        self.assertIs(model.state, ModemState.OFF)

    def test_boot_requires_each_host_startup_service(self):
        services = {
            "ipa_host_ready": "IPA host provider",
            "servreg_mapper_ready": "SERVREG mapper",
            "remote_storage_ready": "remote storage",
            "heap_memory_ready": "heap-memory sharing",
            "tftp_provider_ready": "TFTP provider",
        }
        for field, label in services.items():
            with self.subTest(service=label):
                model = ModemLifecycleModel()
                readiness = {name: True for name in services}
                readiness[field] = False
                with self.assertRaisesRegex(
                    ModemLifecycleError, f"host startup services are not ready: {label}"
                ):
                    model.prepare_boot(
                        firmware_bundle_identified=True,
                        image_layout_valid=True,
                        memory_excluded=True,
                        scm_operation_allowlisted=True,
                        **readiness,
                    )
                self.assertIs(model.state, ModemState.OFF)

    def test_power_auth_and_peer_events_are_ordered(self):
        model = ModemLifecycleModel()
        with self.assertRaises(ModemLifecycleError):
            model.secure_boot_complete(pas_authentication_succeeded=True)
        model = self.prepared()
        with self.assertRaisesRegex(ModemLifecycleError, "AOP"):
            model.power_sequence_complete(aop_sequence_verified=False)
        self.assertIs(model.state, ModemState.QUARANTINED)

    def test_failed_authentication_quarantines_memory(self):
        model = self.prepared()
        model.power_sequence_complete(aop_sequence_verified=True)
        with self.assertRaisesRegex(ModemLifecycleError, "authentication failed"):
            model.secure_boot_complete(pas_authentication_succeeded=False)
        self.assertIs(model.state, ModemState.QUARANTINED)
        with self.assertRaisesRegex(ModemLifecycleError, "remains quarantined"):
            model.confirm_fault_stopped(
                reset_verified=True,
                pas_shutdown_succeeded=False,
                power_down_verified=True,
                transport_dma_contained=True,
            )

    def test_peer_requires_fresh_exclusive_powerup(self):
        model = self.authenticated()
        with self.assertRaisesRegex(ModemLifecycleError, "exclusively owned"):
            model.modem_ready(
                after_powerup_verified=True,
                fresh_reset_verified=False,
                exclusive_transport_verified=True,
            )
        self.assertIs(model.state, ModemState.QUARANTINED)

    def test_wlan_requires_current_peer_and_dma_ownership(self):
        model, peer = self.peer_ready()
        with self.assertRaisesRegex(ModemLifecycleError, "SMMU isolation"):
            model.enable_wlan(peer, msa_owner_verified=True, smmu_isolated=False)
        model.enable_wlan(peer, msa_owner_verified=True, smmu_isolated=True)
        self.assertIs(model.state, ModemState.WLAN_READY)

    def test_stop_invalidates_peer_before_shutdown_ack(self):
        model, peer = self.peer_ready()
        model.request_stop()
        self.assertIsNone(model.peer_generation)
        with self.assertRaisesRegex(ModemLifecycleError, "requires PEER_READY"):
            model.enable_wlan(peer, msa_owner_verified=True, smmu_isolated=True)
        with self.assertRaisesRegex(ModemLifecycleError, "PAS shutdown.*DMA containment"):
            model.shutdown_complete(
                stop_ack_verified=True,
                pas_shutdown_succeeded=True,
                power_down_verified=True,
                transport_dma_contained=False,
            )
        self.assertIs(model.state, ModemState.QUARANTINED)

    def test_successful_stop_allows_a_new_generation(self):
        model, old_peer = self.peer_ready()
        old_generation = old_peer.value
        model.request_stop()
        model.shutdown_complete(
            stop_ack_verified=True,
            pas_shutdown_succeeded=True,
            power_down_verified=True,
            transport_dma_contained=True,
        )
        self.assertIs(model.state, ModemState.OFF)
        self.assertTrue(model.modem_reservation_excluded)
        model.prepare_boot(
            firmware_bundle_identified=True,
            image_layout_valid=True,
            memory_excluded=True,
            scm_operation_allowlisted=True,
            ipa_host_ready=True,
            servreg_mapper_ready=True,
            remote_storage_ready=True,
            heap_memory_ready=True,
            tftp_provider_ready=True,
        )
        model.power_sequence_complete(aop_sequence_verified=True)
        model.secure_boot_complete(pas_authentication_succeeded=True)
        new_peer = model.modem_ready(
            after_powerup_verified=True,
            fresh_reset_verified=True,
            exclusive_transport_verified=True,
        )
        self.assertGreater(new_peer.value, old_generation)
        with self.assertRaisesRegex(ModemLifecycleError, "stale or foreign"):
            model.enable_wlan(old_peer, msa_owner_verified=True, smmu_isolated=True)

    def test_fault_revokes_peer_and_requires_verified_shutdown(self):
        model, peer = self.peer_ready()
        model.fault()
        self.assertIs(model.state, ModemState.QUARANTINED)
        self.assertIsNone(model.peer_generation)
        with self.assertRaises(ModemLifecycleError):
            model.enable_wlan(peer, msa_owner_verified=True, smmu_isolated=True)
        model.confirm_fault_stopped(
            reset_verified=True,
            pas_shutdown_succeeded=True,
            power_down_verified=True,
            transport_dma_contained=True,
        )
        self.assertIs(model.state, ModemState.OFF)
        self.assertTrue(model.modem_reservation_excluded)

    def test_fault_recovery_requires_dma_containment(self):
        model, _peer = self.peer_ready()
        model.fault()
        with self.assertRaisesRegex(ModemLifecycleError, "remains quarantined"):
            model.confirm_fault_stopped(
                reset_verified=True,
                pas_shutdown_succeeded=True,
                power_down_verified=True,
                transport_dma_contained=False,
            )
        self.assertIs(model.state, ModemState.QUARANTINED)


if __name__ == "__main__":
    unittest.main()
