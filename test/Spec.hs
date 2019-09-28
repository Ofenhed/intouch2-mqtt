{-# LANGUAGE OverloadedStrings #-}

import NetworkPackage
import NetworkPackageParser

import Text.ParserCombinators.ReadP
import Test.Hspec
import Test.QuickCheck
import Data.Maybe (maybe)

import qualified Data.ByteString.Char8 as C8

main :: IO ()
main = hspec $ do
  describe "Parser" $ do
    it "Regular ping package without destination" $ do
      let program = readP_to_S parsePackage $ "<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>"
      program `shouldBe` [([AuthorizedNetworkPackage (Just "sender-id") Nothing NetPing], "")]

    it "Regular pong package" $ do
      let program = readP_to_S parsePackage $ "<PACKT><SRCCN>sender-id</SRCCN><DESCN>Somewhere</DESCN><DATAS>APING</DATAS></PACKT>"
      program `shouldBe` [([AuthorizedNetworkPackage (Just "sender-id") (Just "Somewhere") NetPing], "")]

    it "HTML characters in data" $ do
      let program = readP_to_S parsePackage $ "<PACKT><SRCCN>sender-id</SRCCN><DESCN>Somewhere</DESCN><DATAS>1 < 3</DATAS></PACKT>"
      program `shouldBe` [([AuthorizedNetworkPackage (Just "sender-id") (Just "Somewhere") $ NetUnknownPackage "1 < 3"], "")]

    it "Hello package" $ do
      let program = readP_to_S parsePackage $ "<HELLO>1</HELLO>"
      program `shouldBe` [([HelloPackage "1"], "")]

    it "Hello package followed by ping" $ do
      let program = readP_to_S parsePackage $ "<HELLO>1</HELLO><PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>"
      program `shouldBe` [([HelloPackage "1", AuthorizedNetworkPackage (Just "sender-id") Nothing NetPing], "")]

    it "Hello package becomes itself" $ property $ \x -> do
      let package = HelloPackage $ C8.pack x
          program = readP_to_S parsePackage $ show package
      program `shouldBe` [([package], "")]

    it "Authorized package becomes itself" $ property $ \x y z -> do
      let package = AuthorizedNetworkPackage (maybe Nothing (Just . C8.pack) x) (maybe Nothing (Just . C8.pack) y) $ NetUnknownPackage $ C8.pack z
          program = readP_to_S parsePackage $ show package
      program `shouldBe` [([package], "")]
